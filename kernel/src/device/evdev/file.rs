// SPDX-License-Identifier: MPL-2.0

use alloc::sync::Weak;
use core::{
    cmp,
    fmt::Debug,
    sync::atomic::{AtomicU8, AtomicUsize, Ordering},
    time::Duration,
};

use aster_input::{
    event_type_codes::{EventTypes, SynEvent},
    input_dev::InputEvent,
};
use atomic_integer_wrapper::define_atomic_version_of_integer_like_type;
use ostd::{
    mm::{VmReader, VmWriter},
    sync::Mutex,
    Pod,
};

use super::EvdevDevice;
use crate::{
    current_userspace,
    events::IoEvents,
    fs::{
        inode_handle::FileIo,
        utils::{InodeIo, IoctlCmd, StatusFlags},
    },
    prelude::*,
    process::signal::{PollHandle, Pollable, Pollee},
    syscall::ClockId,
    util::ring_buffer::{RbConsumer, RbProducer, RingBuffer},
};

pub(super) const EVDEV_BUFFER_SIZE: usize = 64;

/// Linux evdev driver version returned by `EVIOCGVERSION`.
const EVDEV_DRIVER_VERSION: i32 = 0x010001;

/// EVDEV ioctl variants.
enum EvdevIoctl {
    /// Get device name string (EVIOCGNAME).
    GetName { len: u32 },
    /// Get device physical path string (EVIOCGPHYS).
    GetPhys { len: u32 },
    /// Get device unique identifier string (EVIOCGUNIQ).
    GetUniq { len: u32 },
    /// Get device identification (bus/vendor/product/version) (EVIOCGID).
    GetId,
    /// Get evdev ABI version (EVIOCGVERSION).
    GetVersion,
    /// Get capability bitmap for a given event type, or supported types when type=0 (EVIOCGBIT).
    GetBit { event_type: u32, len: u32 },
    /// Get current key state bitmap (pressed keys) (EVIOCGKEY).
    GetKey { len: u32 },
    /// Get current LED state bitmap (EVIOCGLED).
    GetLed { len: u32 },
    /// Get current switch state bitmap (EVIOCGSW).
    GetSw { len: u32 },
    /// Set event timestamp clock id (EVIOCSCLOCKID).
    SetClockId,
}

impl From<ClockId> for i32 {
    fn from(clock_id: ClockId) -> Self {
        clock_id as i32
    }
}

define_atomic_version_of_integer_like_type!(ClockId, try_from = true, {
    #[derive(Debug)]
    pub(super) struct AtomicClockId(core::sync::atomic::AtomicI32);
});

// Reference: <https://elixir.bootlin.com/linux/v6.17.9/source/include/uapi/linux/input.h#L28>
// Compatible with Linux's event format.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod)]
pub(super) struct EvdevEvent {
    sec: u64,
    usec: u64,
    type_: u16,
    code: u16,
    value: i32,
}

impl EvdevEvent {
    pub(super) fn from_event_and_time(event: &InputEvent, time: Duration) -> Self {
        let (type_, code, value) = event.to_raw();
        Self {
            sec: time.as_secs(),
            usec: time.subsec_micros() as u64,
            type_,
            code,
            value,
        }
    }
}

// Reference: <https://elixir.bootlin.com/linux/v6.17.9/source/drivers/input/evdev.c#L181-L189>
#[repr(u8)]
#[derive(Debug, Clone, Copy, TryFromInt)]
enum EvdevClock {
    Realtime = 0,
    Monotonic = 1,
    Boottime = 2,
}

impl TryFrom<ClockId> for EvdevClock {
    type Error = Error;

    fn try_from(value: ClockId) -> Result<Self> {
        match value {
            ClockId::CLOCK_REALTIME | ClockId::CLOCK_REALTIME_COARSE => Ok(EvdevClock::Realtime),
            ClockId::CLOCK_MONOTONIC | ClockId::CLOCK_MONOTONIC_RAW | ClockId::CLOCK_MONOTONIC_COARSE => {
                Ok(EvdevClock::Monotonic)
            }
            ClockId::CLOCK_BOOTTIME => Ok(EvdevClock::Boottime),
            _ => Err(Error::with_message(
                Errno::EINVAL,
                "clock id not supported for evdev",
            )),
        }
    }
}

impl From<EvdevClock> for u8 {
    fn from(value: EvdevClock) -> Self {
        value as _
    }
}

define_atomic_version_of_integer_like_type!(EvdevClock, try_from = true, {
    #[derive(Debug)]
    struct AtomicEvdevClock(AtomicU8);
});

/// An opened file from an evdev device ([`EvdevDevice`]).
pub(super) struct EvdevFile {
    /// Inner data (shared with the device).
    inner: Arc<EvdevFileInner>,
    /// Weak reference to the evdev device that owns this evdev file.
    evdev: Weak<EvdevDevice>,
}

/// An opened evdev file's inner data (shared with its [`EvdevDevice`]).
pub(super) struct EvdevFileInner {
    /// Consumer for reading events.
    consumer: Mutex<RbConsumer<EvdevEvent>>,
    /// Clock ID for this opened evdev file.
    clock_id: AtomicEvdevClock,
    /// Number of complete event packets available (ended with `SYN_REPORT`).
    packet_count: AtomicUsize,
    /// Pollee for event notification.
    pollee: Pollee,
}

impl Debug for EvdevFile {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.debug_struct("EvdevFile")
            .field("clock_id", &self.inner.clock_id)
            .field("packet_count", &self.inner.packet_count)
            .finish_non_exhaustive()
    }
}

impl EvdevFileInner {
    pub(super) fn read_clock(&self) -> Duration {
        use crate::time::clocks::{BootTimeClock, MonotonicClock, RealTimeClock};

        let clock_id = self.clock_id.load(Ordering::Relaxed);
        match clock_id {
            EvdevClock::Realtime => RealTimeClock::get().read_time(),
            EvdevClock::Monotonic => MonotonicClock::get().read_time(),
            EvdevClock::Boottime => BootTimeClock::get().read_time(),
        }
    }

    pub(super) fn try_clear_with_producer_locked(&self) {
        let Some(mut consumer) = self.consumer.try_lock() else {
            return;
        };

        // Note that the following two operations are racy unless we hold the producer's lock.
        consumer.clear();
        self.packet_count.store(0, Ordering::Relaxed);
        self.pollee.invalidate();
    }

    /// Checks if buffer has complete event packets.
    pub(self) fn has_complete_packets(&self) -> bool {
        self.packet_count.load(Ordering::Relaxed) > 0
    }

    /// Increments the packet count.
    pub(super) fn increment_packet_count(&self) {
        self.packet_count.fetch_add(1, Ordering::Relaxed);
        self.pollee.notify(IoEvents::IN);
    }

    /// Decrements the packet count.
    pub(self) fn decrement_packet_count(&self) {
        if self.packet_count.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.pollee.invalidate();
        }
    }
}

impl EvdevFile {
    pub(super) fn new(
        buffer_size: usize,
        evdev: Weak<EvdevDevice>,
    ) -> (Self, RbProducer<EvdevEvent>) {
        let (producer, consumer) = RingBuffer::new(buffer_size).split();

        let inner = EvdevFileInner {
            consumer: Mutex::new(consumer),
            clock_id: AtomicEvdevClock::new(EvdevClock::Monotonic),
            packet_count: AtomicUsize::new(0),
            pollee: Pollee::new(),
        };
        let evdev_file = Self {
            inner: Arc::new(inner),
            evdev,
        };
        (evdev_file, producer)
    }

    pub(super) fn inner(&self) -> &Arc<EvdevFileInner> {
        &self.inner
    }

    /// Processes events and writes them to the writer.
    ///
    /// Returns the total number of bytes written, or [`Errno::EAGAIN`] if no events are available.
    fn process_events(&self, max_events: usize, writer: &mut VmWriter) -> Result<usize> {
        const EVENT_SIZE: usize = size_of::<EvdevEvent>();

        let mut consumer = self.inner.consumer.lock();
        let mut event_count = 0;

        for _ in 0..max_events {
            let Some(event) = consumer.pop() else {
                break;
            };

            // Update the counter since the event has been consumed.
            if is_syn_report_event(&event) || is_syn_dropped_event(&event) {
                self.inner.decrement_packet_count();
            }

            // Write the event to the writer.
            writer.write_val(&event)?;
            event_count += 1;
        }

        if event_count == 0 {
            return_errno_with_message!(Errno::EAGAIN, "the evdev file has no events");
        }

        Ok(event_count * EVENT_SIZE)
    }

    fn check_io_events(&self) -> IoEvents {
        // TODO: Report `IoEvents::HUP` if the device has been disconnected.

        if self.inner.has_complete_packets() {
            IoEvents::IN
        } else {
            IoEvents::empty()
        }
    }

    fn upgrade_evdev_device(&self) -> Result<Arc<EvdevDevice>> {
        self.evdev
            .upgrade()
            .ok_or_else(|| Error::with_message(Errno::ENODEV, "evdev device is unavailable"))
    }

    fn write_string_to_userspace(&self, value: &str, len: usize, user_ptr: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }

        let mut buffer = vec![0u8; len];
        let bytes = value.as_bytes();
        let copy_len = cmp::min(bytes.len(), len - 1);
        if copy_len > 0 {
            buffer[..copy_len].copy_from_slice(&bytes[..copy_len]);
        }

        let mut reader = VmReader::from(buffer.as_slice());
        current_userspace!().write_bytes(user_ptr, &mut reader)?;
        Ok(())
    }

    fn write_bitmap_to_userspace(&self, bitmap: &[u8], len: usize, user_ptr: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }

        let mut buffer = vec![0u8; len];
        let copy_len = cmp::min(bitmap.len(), len);
        if copy_len > 0 {
            buffer[..copy_len].copy_from_slice(&bitmap[..copy_len]);
        }

        let mut reader = VmReader::from(buffer.as_slice());
        current_userspace!().write_bytes(user_ptr, &mut reader)?;
        Ok(())
    }

    /// Parses raw EVDEV ioctl command into a local variant.
    fn parse_evdev(raw: u32) -> Option<EvdevIoctl> {
        const IOC_NRBITS: u32 = 8;
        const IOC_TYPEBITS: u32 = 8;
        const IOC_SIZEBITS: u32 = 14;
        const IOC_DIRBITS: u32 = 2;

        const IOC_NRMASK: u32 = (1 << IOC_NRBITS) - 1;
        const IOC_TYPEMASK: u32 = (1 << IOC_TYPEBITS) - 1;
        const IOC_SIZEMASK: u32 = (1 << IOC_SIZEBITS) - 1;
        const IOC_DIRMASK: u32 = (1 << IOC_DIRBITS) - 1;

        const IOC_NRSHIFT: u32 = 0;
        const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
        const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
        const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;

        const IOC_READ: u32 = 2;
        const IOC_WRITE: u32 = 1;
        const EVDEV_IOCTL_TYPE: u32 = b'E' as u32;
        const EVIOCGNAME_NR: u32 = 0x06;
        const EVIOCGPHYS_NR: u32 = 0x07;
        const EVIOCGUNIQ_NR: u32 = 0x08;
        const EVIOCGID: u32 = 0x80084502;
        const EVIOCGVERSION: u32 = 0x80044501;
        const EVIOCGBIT_BASE_NR: u32 = 0x20;
        const EVIOCGKEY_NR: u32 = 0x18;
        const EVIOCGLED_NR: u32 = 0x19;
        const EVIOCGSW_NR: u32 = 0x1b;
        const EVIOCSCLOCKID_NR: u32 = 0xa0;

        let dir = (raw >> IOC_DIRSHIFT) & IOC_DIRMASK;
        let type_ = (raw >> IOC_TYPESHIFT) & IOC_TYPEMASK;
        let nr = (raw >> IOC_NRSHIFT) & IOC_NRMASK;
        let len = (raw >> IOC_SIZESHIFT) & IOC_SIZEMASK;

        if type_ != EVDEV_IOCTL_TYPE {
            return None;
        }

        if raw == EVIOCGVERSION {
            return Some(EvdevIoctl::GetVersion);
        }
        if raw == EVIOCGID {
            return Some(EvdevIoctl::GetId);
        }

        match dir {
            IOC_READ => match nr {
                EVIOCGNAME_NR => Some(EvdevIoctl::GetName { len }),
                EVIOCGPHYS_NR => Some(EvdevIoctl::GetPhys { len }),
                EVIOCGUNIQ_NR => Some(EvdevIoctl::GetUniq { len }),
                EVIOCGKEY_NR => Some(EvdevIoctl::GetKey { len }),
                EVIOCGLED_NR => Some(EvdevIoctl::GetLed { len }),
                EVIOCGSW_NR => Some(EvdevIoctl::GetSw { len }),
                n if n >= EVIOCGBIT_BASE_NR => Some(EvdevIoctl::GetBit {
                    event_type: n - EVIOCGBIT_BASE_NR,
                    len,
                }),
                _ => None,
            },
            IOC_WRITE => match nr {
                EVIOCSCLOCKID_NR => Some(EvdevIoctl::SetClockId),
                _ => None,
            },
            _ => None,
        }
    }

    fn handle_evdev_ioctl(&self, raw: u32, arg: usize) -> Result<()> {
        match Self::parse_evdev(raw) {
            Some(EvdevIoctl::GetName { len }) => {
                let evdev = self.upgrade_evdev_device()?;
                self.write_string_to_userspace(evdev.device.name(), len as usize, arg)?;
            }
            Some(EvdevIoctl::GetPhys { len }) => {
                let evdev = self.upgrade_evdev_device()?;
                self.write_string_to_userspace(evdev.device.phys(), len as usize, arg)?;
            }
            Some(EvdevIoctl::GetUniq { len }) => {
                let evdev = self.upgrade_evdev_device()?;
                self.write_string_to_userspace(evdev.device.uniq(), len as usize, arg)?;
            }
            Some(EvdevIoctl::GetId) => {
                let evdev = self.upgrade_evdev_device()?;
                let id = evdev.device.id();
                current_userspace!().write_val(arg, &id)?;
            }
            Some(EvdevIoctl::GetVersion) => {
                current_userspace!().write_val(arg, &EVDEV_DRIVER_VERSION)?;
            }
            Some(EvdevIoctl::GetBit { event_type, len }) => {
                let evdev = self.upgrade_evdev_device()?;
                let capability = evdev.device.capability();
                let event_types_bytes = capability.event_types_bits().to_le_bytes();
                let bitmap = match event_type as u16 {
                    0 => Some(&event_types_bytes[..]),
                    t if t == EventTypes::KEY.as_index() => {
                        Some(capability.supported_keys_bitmap())
                    }
                    t if t == EventTypes::REL.as_index() => {
                        Some(capability.supported_relative_axes_bitmap())
                    }
                    _ => None,
                };
                let bitmap = bitmap.unwrap_or(&[]);
                self.write_bitmap_to_userspace(bitmap, len as usize, arg)?;
            }
            Some(EvdevIoctl::GetKey { len })
            | Some(EvdevIoctl::GetLed { len })
            | Some(EvdevIoctl::GetSw { len }) => {
                // TODO: These states are not maintained yet, and libevdev only checks for a zero return value,
                // so we provide a temporary dummy implementation.
                let zero = vec![0u8; len as usize];
                self.write_bitmap_to_userspace(&zero[..], len as usize, arg)?;
            }
            Some(EvdevIoctl::SetClockId) => {
                let clock_id_raw: i32 = current_userspace!().read_val(arg)?;
                let clock_id = ClockId::try_from(clock_id_raw)
                    .map_err(|_| Error::with_message(Errno::EINVAL, "invalid clock id"))?;

                let evdev_clock = EvdevClock::try_from(clock_id)?;

                self.inner.clock_id.store(evdev_clock, Ordering::Relaxed);
            }
            None => {
                return Err(Error::with_message(
                    Errno::EINVAL,
                    "This IOCTL operation not supported on evdev devices",
                ))
            }
        }

        Ok(())
    }
}

/// Checks if the event is a `SYN_REPORT` event.
pub(super) fn is_syn_report_event(event: &EvdevEvent) -> bool {
    event.type_ == EventTypes::SYN.as_index() && event.code == SynEvent::Report as u16
}

/// Checks if the event is a `SYN_DROPPED` event.
pub(super) fn is_syn_dropped_event(event: &EvdevEvent) -> bool {
    event.type_ == EventTypes::SYN.as_index() && event.code == SynEvent::Dropped as u16
}

impl Pollable for EvdevFile {
    fn poll(&self, mask: IoEvents, poller: Option<&mut PollHandle>) -> IoEvents {
        self.inner
            .pollee
            .poll_with(mask, poller, || self.check_io_events())
    }
}

impl InodeIo for EvdevFile {
    fn read_at(
        &self,
        _offset: usize,
        writer: &mut VmWriter,
        status_flags: StatusFlags,
    ) -> Result<usize> {
        let requested_bytes = writer.avail();
        let max_events = requested_bytes / size_of::<EvdevEvent>();

        if max_events == 0 && requested_bytes != 0 {
            return_errno_with_message!(Errno::EINVAL, "the buffer is too short");
        }

        // TODO: Return `ENODEV` if the device has been disconnected.

        let is_nonblocking = status_flags.contains(StatusFlags::O_NONBLOCK);

        // If we're in non-blocking mode, we won't bother the user space with an incomplete packet.
        // Note that this aligns to the behavior of `check_io_events`.
        if is_nonblocking && !self.inner.has_complete_packets() {
            return_errno_with_message!(Errno::EAGAIN, "the evdev file has no packets");
        }

        // Even if `max_events` is zero, the above checks are still needed.
        if max_events == 0 {
            return Ok(0);
        }

        if is_nonblocking {
            self.process_events(max_events, writer)
        } else {
            self.wait_events(IoEvents::IN, None, || {
                self.process_events(max_events, writer)
            })
        }
    }

    fn write_at(
        &self,
        _offset: usize,
        _reader: &mut VmReader,
        _status_flags: StatusFlags,
    ) -> Result<usize> {
        // TODO: In Linux, writing to evdev files is permitted and will inject input events.
        return_errno_with_message!(Errno::ENOSYS, "writing to evdev files is not supported yet");
    }
}

impl FileIo for EvdevFile {
    fn check_seekable(&self) -> Result<()> {
        return_errno_with_message!(Errno::ESPIPE, "the inode is an evdev file");
    }

    fn is_offset_aware(&self) -> bool {
        false
    }

    fn ioctl(&self, cmd: IoctlCmd, arg: usize) -> Result<i32> {
        match cmd {
            IoctlCmd::Others(raw) => self.handle_evdev_ioctl(raw, arg)?,
            _ => {
                return_errno!(Errno::EINVAL)
            }
        }

        Ok(0)
    }
}

impl Drop for EvdevFile {
    fn drop(&mut self) {
        if let Some(evdev) = self.evdev.upgrade() {
            evdev.detach_closed_file(&self.inner);
        }
    }
}
