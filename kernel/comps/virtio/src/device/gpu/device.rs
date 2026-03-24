// SPDX-License-Identifier: MPL-2.0

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::{cmp::min, hint::spin_loop, mem::size_of, sync::atomic::{AtomicU32, AtomicU64, Ordering}};

use aster_gpu::GpuDevice;
use aster_util::mem_obj_slice::Slice;
use bitflags::bitflags;
use log::{debug, info, warn};
use ostd::{
    Pod,
    arch::trap::TrapFrame,
    mm::{HasSize, PAGE_SIZE, VmIo, dma::DmaStream},
    sync::SpinLock,
};
use ostd::prelude::println;
use super::{CMD_CTX_CREATE, CMD_CTX_DESTROY, CMD_GET_CAPSET, CMD_GET_CAPSET_INFO,
    CMD_GET_DISPLAY_INFO, CMD_GET_EDID, CMD_RESOURCE_ATTACH_BACKING,
    CMD_RESOURCE_CREATE_2D, CMD_RESOURCE_CREATE_3D, CMD_RESOURCE_CREATE_BLOB, CMD_RESOURCE_UNREF,
    CMD_RESOURCE_DETACH_BACKING, CMD_RESOURCE_FLUSH, CMD_TRANSFER_FROM_HOST_3D,
    CMD_TRANSFER_TO_HOST_2D, CMD_TRANSFER_TO_HOST_3D, CMD_SET_SCANOUT, CMD_SUBMIT_3D,
    DEVICE_NAME, GpuFeatures, QUEUE_CONTROL,
    QUEUE_CURSOR, RESP_OK_CAPSET, RESP_OK_CAPSET_INFO, RESP_OK_DISPLAY_INFO, RESP_OK_EDID, RESP_OK_NODATA,
    VirtioGpuConfig, VirtioGpuCtrlHdr, VirtioGpuDisplayOne, VirtioGpuFormat, VirtioGpuGetCapset, VirtioGpuGetCapsetInfo,
    VirtioGpuCtxCreate, VirtioGpuCtxDestroy, VirtioGpuGetEdid, VirtioGpuMemEntry, VirtioGpuRect, VirtioGpuResourceAttachBacking,
    VirtioGpuResourceUnref, VirtioGpuResourceDetachBacking,
    VirtioGpuResourceCreate2d, VirtioGpuResourceCreate3d, VirtioGpuResourceCreateBlob, VirtioGpuRespCapsetInfo, VirtioGpuRespDisplayInfo,
    VirtioGpuRespEdid, VirtioGpuResourceFlush, VirtioGpuTransferHost3d, VirtioGpuTransferToHost2d, VirtioGpuSetScanout, VirtioGpuCmdSubmit,
};
use crate::{
    device::{VirtioDeviceError, gpu::drm::VirtioGpuDrmDrvier},
    id_alloc::SyncIdAlloc,
    queue::{QueueError, VirtQueue},
    transport::{ConfigManager, VirtioTransport},
};

const CTRL_QUEUE_SIZE: u16 = 64;
const CTRL_REQ_STRIDE: usize = {
    let mut max = size_of::<VirtioGpuCtrlHdr>();
    if size_of::<VirtioGpuGetCapsetInfo>() > max {
        max = size_of::<VirtioGpuGetCapsetInfo>();
    }
    if size_of::<VirtioGpuGetCapset>() > max {
        max = size_of::<VirtioGpuGetCapset>();
    }
    if size_of::<VirtioGpuGetEdid>() > max {
        max = size_of::<VirtioGpuGetEdid>();
    }
    if size_of::<VirtioGpuResourceCreate2d>() > max {
        max = size_of::<VirtioGpuResourceCreate2d>();
    }
    if size_of::<VirtioGpuResourceCreate3d>() > max {
        max = size_of::<VirtioGpuResourceCreate3d>();
    }
    if size_of::<VirtioGpuResourceUnref>() > max {
        max = size_of::<VirtioGpuResourceUnref>();
    }
    if size_of::<VirtioGpuResourceAttachBacking>() > max {
        max = size_of::<VirtioGpuResourceAttachBacking>();
    }
    if size_of::<VirtioGpuResourceDetachBacking>() > max {
        max = size_of::<VirtioGpuResourceDetachBacking>();
    }
    if size_of::<VirtioGpuResourceFlush>() > max {
        max = size_of::<VirtioGpuResourceFlush>();
    }
    if size_of::<VirtioGpuTransferToHost2d>() > max {
        max = size_of::<VirtioGpuTransferToHost2d>();
    }
    if size_of::<VirtioGpuTransferHost3d>() > max {
        max = size_of::<VirtioGpuTransferHost3d>();
    }
    if size_of::<VirtioGpuSetScanout>() > max {
        max = size_of::<VirtioGpuSetScanout>();
    }
    if size_of::<VirtioGpuCmdSubmit>() > max {
        max = size_of::<VirtioGpuCmdSubmit>();
    }
    if size_of::<VirtioGpuCtxCreate>() > max {
        max = size_of::<VirtioGpuCtxCreate>();
    }
    if size_of::<VirtioGpuCtxDestroy>() > max {
        max = size_of::<VirtioGpuCtxDestroy>();
    }
    max
};
const CTRL_RESP_STRIDE: usize = size_of::<VirtioGpuRespEdid>();
const VIRTIO_RING_F_INDIRECT_DESC: u64 = 1 << 28;
const VIRTIO_GPU_FLAG_FENCE: u32 = 1 << 0;
const VIRTIO_GPU_FLAG_INFO_RING_IDX: u32 = 1 << 1;

bitflags! {
    struct VirtioGpuCaps: u32 {
        const VIRGL_3D = 1 << 0;
        const EDID = 1 << 1;
        const INDIRECT_DESC = 1 << 2;
        const RESOURCE_ASSIGN_UUID = 1 << 3;
        const RESOURCE_BLOB = 1 << 4;
        const CONTEXT_INIT = 1 << 5;
    }
}

#[derive(Debug)]
pub struct VirtioGpuDevice {
    config_manager: ConfigManager<VirtioGpuConfig>,
    control_queue: SpinLock<VirtQueue>,
    cursor_queue: SpinLock<VirtQueue>,
    transport: SpinLock<Box<dyn VirtioTransport>>,
    ctrl_requests: Arc<DmaStream>,
    ctrl_responses: Arc<DmaStream>,
    id_allocator: SyncIdAlloc,
    next_resource_id: AtomicU32,
    next_context_id: AtomicU32,
    next_fence_id: AtomicU64,
    caps: VirtioGpuCaps,
    num_scanouts: SpinLock<u32>,
    display_infos: SpinLock<Vec<VirtioGpuDisplayOne>>,
    display_info_resp: SpinLock<Option<VirtioGpuRespDisplayInfo>>,
    edids: SpinLock<Vec<Option<VirtioGpuRespEdid>>>,
    num_capsets: SpinLock<u32>,
    capset_infos: SpinLock<Vec<VirtioGpuRespCapsetInfo>>,
}

impl VirtioGpuDevice {
    pub(crate) fn negotiate_features(device_features: u64) -> u64 {
        let supported_features = GpuFeatures::VIRGL
            | GpuFeatures::EDID
            | GpuFeatures::RESOURCE_UUID
            | GpuFeatures::RESOURCE_BLOB
            | GpuFeatures::CONTEXT_INIT;
        (GpuFeatures::from_bits_truncate(device_features) & supported_features).bits()
    }

    pub(crate) fn init(mut transport: Box<dyn VirtioTransport>) -> Result<(), VirtioDeviceError> {
        let num_queues = transport.num_queues();
        if num_queues < 2 {
            return Err(VirtioDeviceError::QueuesAmountDoNotMatch(num_queues, 2));
        }

        let mut control_queue = VirtQueue::new(QUEUE_CONTROL, CTRL_QUEUE_SIZE, transport.as_mut())
            .expect("create virtio-gpu control queue failed");
        let cursor_queue = VirtQueue::new(QUEUE_CURSOR, CTRL_QUEUE_SIZE, transport.as_mut())
            .expect("create virtio-gpu cursor queue failed");

        // We currently use synchronous command submission during initialization,
        // so queue callbacks are not required yet.
        control_queue.disable_callback();

        let ctrl_req_frames = (CTRL_REQ_STRIDE * CTRL_QUEUE_SIZE as usize).div_ceil(PAGE_SIZE);
        let ctrl_resp_frames = (CTRL_RESP_STRIDE * CTRL_QUEUE_SIZE as usize).div_ceil(PAGE_SIZE);

        let ctrl_requests = Arc::new(DmaStream::alloc(ctrl_req_frames, false).unwrap());
        let ctrl_responses = Arc::new(DmaStream::alloc(ctrl_resp_frames, false).unwrap());
        assert!(CTRL_REQ_STRIDE * CTRL_QUEUE_SIZE as usize <= ctrl_requests.size());
        assert!(CTRL_RESP_STRIDE * CTRL_QUEUE_SIZE as usize <= ctrl_responses.size());

        let device_features = transport.read_device_features();
        let gpu_features = GpuFeatures::from_bits_truncate(device_features);
        let mut caps = VirtioGpuCaps::empty();
        if cfg!(target_endian = "little") && gpu_features.contains(GpuFeatures::VIRGL) {
            caps.insert(VirtioGpuCaps::VIRGL_3D);
        }
        if gpu_features.contains(GpuFeatures::EDID) {
            caps.insert(VirtioGpuCaps::EDID);
        }
        if (device_features & VIRTIO_RING_F_INDIRECT_DESC) != 0 {
            caps.insert(VirtioGpuCaps::INDIRECT_DESC);
        }
        if gpu_features.contains(GpuFeatures::RESOURCE_UUID) {
            caps.insert(VirtioGpuCaps::RESOURCE_ASSIGN_UUID);
        }
        if gpu_features.contains(GpuFeatures::RESOURCE_BLOB) {
            caps.insert(VirtioGpuCaps::RESOURCE_BLOB);
        }
        if gpu_features.contains(GpuFeatures::CONTEXT_INIT) {
            caps.insert(VirtioGpuCaps::CONTEXT_INIT);
        }

        let config_manager = VirtioGpuConfig::new_manager(transport.as_ref());

        // Read initial config so we can initialize spinlocks with sensible
        // defaults derived from device config (avoids temporarily showing
        // zero values before config is read from the device).
        let initial_config = config_manager.read_config();

        let device = Arc::new(VirtioGpuDevice {
            config_manager,
            control_queue: SpinLock::new(control_queue),
            cursor_queue: SpinLock::new(cursor_queue),
            transport: SpinLock::new(transport),
            ctrl_requests,
            ctrl_responses,
            id_allocator: SyncIdAlloc::with_capacity(CTRL_QUEUE_SIZE as usize),
            next_resource_id: AtomicU32::new(1),
            next_context_id: AtomicU32::new(1),
            next_fence_id: AtomicU64::new(1),
            caps,
            num_scanouts: SpinLock::new(initial_config.num_scanouts),
            display_infos: SpinLock::new(Vec::new()),
            display_info_resp: SpinLock::new(None),
            edids: SpinLock::new(Vec::new()),
            num_capsets: SpinLock::new(initial_config.num_capsets),
            capset_infos: SpinLock::new(Vec::new()),
        });

        {
            fn config_space_change(_: &TrapFrame) {
                debug!("virtio-gpu config space changed");
            }

            let mut transport = device.transport.lock();
            transport
                .register_cfg_callback(Box::new(config_space_change))
                .unwrap();
            transport.finish_init();
        }

        // Register this bus-level GPU device to the common GPU subsystem.
        if let Err(err) = aster_gpu::register_device(device.clone()) {
            warn!(
                "failed to register virtio-gpu device into gpu subsystem: {:?}",
                err
            );
        }

        // Register this bus-level DRM driver to the common GPU subsystem.
        if let Err(err) = aster_gpu::register_driver(DEVICE_NAME, Arc::new(VirtioGpuDrmDrvier)) {
            warn!(
                "failed to register virtio-gpu device into gpu subsystem: {:?}",
                err
            );
        }

        let config = device.config_manager.read_config();
        info!(
            "virtio-gpu initialized: scanouts={}, capsets={}",
            config.num_scanouts, config.num_capsets
        );
        info!(
            "virtio-gpu features: virgl_3d={}, edid={}, indirect_desc={}, resource_uuid={}, resource_blob={}, context_init={}",
            device.has_virgl_3d(),
            device.has_edid(),
            device.has_indirect(),
            device.has_resource_assign_uuid(),
            device.has_resource_blob(),
            device.has_context_init()
        );
        *device.num_scanouts.lock() = config.num_scanouts;
        *device.num_capsets.lock() = config.num_capsets;

        let mut capset_infos = Vec::new();
        if config.num_capsets > 0 {
            for capset_index in 0..config.num_capsets {
                match device.get_capset_info(capset_index) {
                    Ok(info) => {
                        info!(
                            "virtio-gpu capset[{capset_index}]: id={}, max_version={}, max_size={}",
                            info.capset_id, info.capset_max_version, info.capset_max_size
                        );
                        capset_infos.push(info);
                    }
                    Err(err) => {
                        warn!(
                            "virtio-gpu get capset info failed at index {capset_index}: {:?}",
                            err
                        );
                    }
                }
            }
        }
        *device.capset_infos.lock() = capset_infos;

        let mut display_infos = Vec::new();
        let mut edids = vec![None; config.num_scanouts as usize];

        if config.num_scanouts > 0 {
            if device.has_edid() {
                for scanout in 0..config.num_scanouts {
                    match device.get_edid(scanout) {
                        Ok(edid) => {
                            let edid_size = min(edid.size as usize, edid.edid.len());
                            info!("virtio-gpu edid fetched: scanout={scanout}, size={edid_size}");
                            edids[scanout as usize] = Some(edid);
                        }
                        Err(err) => {
                            warn!("virtio-gpu get edid failed at scanout {scanout}: {:?}", err);
                        }
                    }
                }
            }

            match device.get_display_info() {
                Ok(resp) => {
                    *device.display_info_resp.lock() = Some(resp);
                    let scanout_count = min(config.num_scanouts as usize, resp.pmodes.len());
                    display_infos.extend_from_slice(&resp.pmodes[..scanout_count]);

                    let active_scanouts = display_infos
                        .iter()
                        .filter(|mode| {
                            mode.enabled != 0 && mode.rect.width != 0 && mode.rect.height != 0
                        })
                        .count();
                    info!("virtio-gpu display info fetched, active scanouts={active_scanouts}");
                }
                Err(err) => {
                    warn!("virtio-gpu get display info failed: {:?}", err);
                }
            }
        }

        *device.display_infos.lock() = display_infos;
        *device.edids.lock() = edids;

        Ok(())
    }
}

impl GpuDevice for VirtioGpuDevice {
    fn driver_name(&self) -> &str {
        DEVICE_NAME
    }
}

impl VirtioGpuDevice {
    fn alloc_fence_id(&self) -> u64 {
        self.next_fence_id.fetch_add(1, Ordering::Relaxed)
    }

    fn submit_control_dma_buffers(
        &self,
        req_buffers: &[&Slice<Arc<DmaStream>>],
        resp_buffers: &[&Slice<Arc<DmaStream>>],
    ) -> Result<(), VirtioGpuCommandError> {
        let needed_desc = req_buffers.len() + resp_buffers.len();

        let token = loop {
            let mut queue = self.control_queue.disable_irq().lock();
            if queue.available_desc() >= needed_desc {
                let token = queue
                    .add_dma_buf(req_buffers, resp_buffers)
                    .map_err(VirtioGpuCommandError::Queue)?;
                if queue.should_notify() {
                    queue.notify();
                }
                break token;
            }
            drop(queue);
            spin_loop();
        };

        loop {
            let mut queue = self.control_queue.disable_irq().lock();
            if !queue.can_pop() {
                drop(queue);
                spin_loop();
                continue;
            }
            queue
                .pop_used_with_token(token)
                .map_err(VirtioGpuCommandError::Queue)?;
            break;
        }

        Ok(())
    }

    fn stamp_fence(&self, req_slice: &Slice<Arc<DmaStream>>, fence_id: u64) {
        let mut hdr: VirtioGpuCtrlHdr = req_slice.read_val(0).unwrap();
        hdr.flags |= VIRTIO_GPU_FLAG_FENCE;
        hdr.fence_id = fence_id;
        req_slice.write_val(0, &hdr).unwrap();
        req_slice.sync_to_device().unwrap();
    }

    pub fn alloc_resource_id(&self) -> u32 {
        self.next_resource_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn alloc_context_id(&self) -> u32 {
        self.next_context_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn context_create(
        &self,
        context_id: u32,
        context_init: u32,
        debug_name: &[u8],
    ) -> Result<(), VirtioGpuCommandError> {
        if debug_name.len() > 64 {
            return Err(VirtioGpuCommandError::InvalidParameter);
        }

        let mut req = VirtioGpuCtxCreate {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_CTX_CREATE,
                ctx_id: context_id,
                ..Default::default()
            },
            nlen: debug_name.len() as u32,
            context_init,
            debug_name: [0; 64],
        };
        req.debug_name[..debug_name.len()].copy_from_slice(debug_name);

        let _: VirtioGpuCtrlHdr =
            self.submit_control_command::<VirtioGpuCtxCreate, VirtioGpuCtrlHdr>(&req, RESP_OK_NODATA)?;
        Ok(())
    }

    pub fn context_destroy(&self, context_id: u32) -> Result<(), VirtioGpuCommandError> {
        let req = VirtioGpuCtxDestroy {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_CTX_DESTROY,
                ctx_id: context_id,
                ..Default::default()
            },
        };

        let _: VirtioGpuCtrlHdr =
            self.submit_control_command::<VirtioGpuCtxDestroy, VirtioGpuCtrlHdr>(&req, RESP_OK_NODATA)?;
        Ok(())
    }

    pub fn get_display_info(&self) -> Result<VirtioGpuRespDisplayInfo, VirtioGpuCommandError> {
        let req = VirtioGpuCtrlHdr {
            type_: CMD_GET_DISPLAY_INFO,
            ..Default::default()
        };
        self.submit_control_command::<VirtioGpuCtrlHdr, VirtioGpuRespDisplayInfo>(
            &req,
            RESP_OK_DISPLAY_INFO,
        )
    }

    pub fn get_capset_info(
        &self,
        capset_index: u32,
    ) -> Result<VirtioGpuRespCapsetInfo, VirtioGpuCommandError> {
        let req = VirtioGpuGetCapsetInfo {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_GET_CAPSET_INFO,
                ..Default::default()
            },
            capset_index,
            padding: 0,
        };
        self.submit_control_command::<VirtioGpuGetCapsetInfo, VirtioGpuRespCapsetInfo>(
            &req,
            RESP_OK_CAPSET_INFO,
        )
    }

    pub fn get_capset(
        &self,
        capset_id: u32,
        capset_version: u32,
        capset_size: u32,
    ) -> Result<Vec<u8>, VirtioGpuCommandError> {
        let req = VirtioGpuGetCapset {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_GET_CAPSET,
                ..Default::default()
            },
            capset_id,
            capset_version,
        };

        let resp_len = size_of::<VirtioGpuCtrlHdr>()
            + usize::try_from(capset_size).map_err(|_| VirtioGpuCommandError::InvalidParameter)?;
        let resp_frames = resp_len.div_ceil(PAGE_SIZE);
        let resp_dma = Arc::new(DmaStream::alloc(resp_frames, false).unwrap());
        let resp_slice = Slice::new(resp_dma, 0..resp_len);
        resp_slice
            .write_val(0, &VirtioGpuCtrlHdr::default())
            .unwrap();
        resp_slice.sync_to_device().unwrap();

        let req_len = size_of::<VirtioGpuGetCapset>();
        let req_frames = req_len.div_ceil(PAGE_SIZE);
        let req_dma = Arc::new(DmaStream::alloc(req_frames, false).unwrap());
        let req_slice = Slice::new(req_dma, 0..req_len);
        req_slice.write_val(0, &req).unwrap();
        let fence_id = self.alloc_fence_id();
        self.stamp_fence(&req_slice, fence_id);

        self.submit_control_dma_buffers(&[&req_slice], &[&resp_slice])?;

        resp_slice.sync_from_device().unwrap();
        let resp_hdr: VirtioGpuCtrlHdr = resp_slice.read_val(0).unwrap();
        if resp_hdr.type_ != RESP_OK_CAPSET {
            return Err(VirtioGpuCommandError::UnexpectedResponse(resp_hdr.type_));
        }
        if resp_hdr.fence_id != fence_id {
            return Err(VirtioGpuCommandError::FenceMismatch {
                expected: fence_id,
                got: resp_hdr.fence_id,
            });
        }

        let data_offset = size_of::<VirtioGpuCtrlHdr>();
        let mut data = vec![0; usize::try_from(capset_size).unwrap()];
        resp_slice.read_bytes(data_offset, &mut data).unwrap();
        Ok(data)
    }

    pub fn get_edid(&self, scanout: u32) -> Result<VirtioGpuRespEdid, VirtioGpuCommandError> {
        let req = VirtioGpuGetEdid {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_GET_EDID,
                ..Default::default()
            },
            scanout,
            padding: 0,
        };
        self.submit_control_command::<VirtioGpuGetEdid, VirtioGpuRespEdid>(&req, RESP_OK_EDID)
    }

    pub fn resource_create_2d(
        &self,
        resource_id: u32,
        format: u32,
        width: u32,
        height: u32,
    ) -> Result<(), VirtioGpuCommandError> {
        let _ = self.resource_create_2d_with_fence(resource_id, format, width, height)?;
        Ok(())
    }

    pub fn resource_create_2d_with_fence(
        &self,
        resource_id: u32,
        format: u32,
        width: u32,
        height: u32,
    ) -> Result<VirtioGpuCtrlHdr, VirtioGpuCommandError> {
        let req = VirtioGpuResourceCreate2d {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_RESOURCE_CREATE_2D,
                ..Default::default()
            },
            resource_id,
            format,
            width,
            height,
        };
        let (resp, _) = self
            .submit_control_command_with_fence::<VirtioGpuResourceCreate2d, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(resp)
    }

    pub fn resource_create_3d(
        &self,
        resource_id: u32,
        target: u32,
        format: u32,
        bind: u32,
        width: u32,
        height: u32,
        depth: u32,
        array_size: u32,
        last_level: u32,
        nr_samples: u32,
        flags: u32,
    ) -> Result<(), VirtioGpuCommandError> {
        let _ = self.resource_create_3d_with_fence(
            resource_id,
            target,
            format,
            bind,
            width,
            height,
            depth,
            array_size,
            last_level,
            nr_samples,
            flags,
        )?;
        Ok(())
    }

    pub fn resource_create_3d_with_fence(
        &self,
        resource_id: u32,
        target: u32,
        format: u32,
        bind: u32,
        width: u32,
        height: u32,
        depth: u32,
        array_size: u32,
        last_level: u32,
        nr_samples: u32,
        flags: u32,
    ) -> Result<VirtioGpuCtrlHdr, VirtioGpuCommandError> {
        let req = VirtioGpuResourceCreate3d {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_RESOURCE_CREATE_3D,
                ..Default::default()
            },
            resource_id,
            target,
            format,
            bind,
            width,
            height,
            depth,
            array_size,
            last_level,
            nr_samples,
            flags,
            padding: 0,
        };

        let (resp, _) = self
            .submit_control_command_with_fence::<VirtioGpuResourceCreate3d, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(resp)
    }

    pub fn resource_attach_backing(
        &self,
        resource_id: u32,
        addr: u64,
        length: u32,
    ) -> Result<(), VirtioGpuCommandError> {
        let req = VirtioGpuResourceAttachBacking {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_RESOURCE_ATTACH_BACKING,
                ..Default::default()
            },
            resource_id,
            nr_entries: 1,
            entries: [VirtioGpuMemEntry {
                addr,
                length,
                padding: 0,
            }],
        };
        let _: VirtioGpuCtrlHdr = self
            .submit_control_command::<VirtioGpuResourceAttachBacking, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(())
    }

    pub fn resource_unref(&self, resource_id: u32) -> Result<(), VirtioGpuCommandError> {
        let req = VirtioGpuResourceUnref {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_RESOURCE_UNREF,
                ..Default::default()
            },
            resource_id,
            padding: 0,
        };
        let _: VirtioGpuCtrlHdr = self
            .submit_control_command::<VirtioGpuResourceUnref, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(())
    }

    pub fn resource_detach_backing(
        &self,
        resource_id: u32,
    ) -> Result<(), VirtioGpuCommandError> {
        let req = VirtioGpuResourceDetachBacking {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_RESOURCE_DETACH_BACKING,
                ..Default::default()
            },
            resource_id,
            padding: 0,
        };
        let _: VirtioGpuCtrlHdr = self
            .submit_control_command::<VirtioGpuResourceDetachBacking, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(())
    }

    pub fn resource_attach_backing_sg(
        &self,
        resource_id: u32,
        entries: &[VirtioGpuMemEntry],
    ) -> Result<(), VirtioGpuCommandError> {
        if entries.is_empty() {
            return Err(VirtioGpuCommandError::InvalidParameter);
        }

        let nr_entries =
            u32::try_from(entries.len()).map_err(|_| VirtioGpuCommandError::InvalidParameter)?;

        let hdr = VirtioGpuResourceAttachBackingHdr {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_RESOURCE_ATTACH_BACKING,
                ..Default::default()
            },
            resource_id,
            nr_entries,
        };

        let req_len = size_of::<VirtioGpuResourceAttachBackingHdr>()
            + entries.len() * size_of::<VirtioGpuMemEntry>();
        let req_frames = req_len.div_ceil(PAGE_SIZE);
        let req_dma = Arc::new(DmaStream::alloc(req_frames, false).unwrap());
        let req_slice = Slice::new(req_dma, 0..req_len);

        req_slice.write_val(0, &hdr).unwrap();
        let mut off = size_of::<VirtioGpuResourceAttachBackingHdr>();
        for entry in entries {
            req_slice.write_val(off, entry).unwrap();
            off += size_of::<VirtioGpuMemEntry>();
        }
        let fence_id = self.alloc_fence_id();
        self.stamp_fence(&req_slice, fence_id);

        let resp_len = size_of::<VirtioGpuCtrlHdr>();
        let resp_frames = resp_len.div_ceil(PAGE_SIZE);
        let resp_dma = Arc::new(DmaStream::alloc(resp_frames, false).unwrap());
        let resp_slice = Slice::new(resp_dma, 0..resp_len);
        resp_slice
            .write_val(0, &VirtioGpuCtrlHdr::default())
            .unwrap();
        resp_slice.sync_to_device().unwrap();

        self.submit_control_dma_buffers(&[&req_slice], &[&resp_slice])?;

        resp_slice.sync_from_device().unwrap();
        let resp_hdr: VirtioGpuCtrlHdr = resp_slice.read_val(0).unwrap();
        if resp_hdr.type_ != RESP_OK_NODATA {
            return Err(VirtioGpuCommandError::UnexpectedResponse(resp_hdr.type_));
        }
        if resp_hdr.fence_id != fence_id {
            return Err(VirtioGpuCommandError::FenceMismatch {
                expected: fence_id,
                got: resp_hdr.fence_id,
            });
        }

        Ok(())
    }

    pub fn resource_create_blob(
        &self,
        resource_id: u32,
        blob_mem: u32,
        blob_flags: u32,
        blob_id: u64,
        size: u64,
        ctx_id: u32,
        entries: &[VirtioGpuMemEntry],
    ) -> Result<(), VirtioGpuCommandError> {
        let _ = self.resource_create_blob_with_fence(
            resource_id,
            blob_mem,
            blob_flags,
            blob_id,
            size,
            ctx_id,
            entries,
        )?;
        Ok(())
    }

    pub fn resource_create_blob_with_fence(
        &self,
        resource_id: u32,
        blob_mem: u32,
        blob_flags: u32,
        blob_id: u64,
        size: u64,
        ctx_id: u32,
        entries: &[VirtioGpuMemEntry],
    ) -> Result<VirtioGpuCtrlHdr, VirtioGpuCommandError> {
        let nr_entries =
            u32::try_from(entries.len()).map_err(|_| VirtioGpuCommandError::InvalidParameter)?;

        let req = VirtioGpuResourceCreateBlob {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_RESOURCE_CREATE_BLOB,
                ctx_id,
                ..Default::default()
            },
            resource_id,
            blob_mem,
            blob_flags,
            nr_entries,
            blob_id,
            size,
        };

        let req_len = size_of::<VirtioGpuResourceCreateBlob>()
            + entries.len() * size_of::<VirtioGpuMemEntry>();
        let req_frames = req_len.div_ceil(PAGE_SIZE);
        let req_dma = Arc::new(DmaStream::alloc(req_frames, false).unwrap());
        let req_slice = Slice::new(req_dma, 0..req_len);

        req_slice.write_val(0, &req).unwrap();
        let mut off = size_of::<VirtioGpuResourceCreateBlob>();
        for entry in entries {
            req_slice.write_val(off, entry).unwrap();
            off += size_of::<VirtioGpuMemEntry>();
        }
        let fence_id = self.alloc_fence_id();
        self.stamp_fence(&req_slice, fence_id);

        let resp_len = size_of::<VirtioGpuCtrlHdr>();
        let resp_frames = resp_len.div_ceil(PAGE_SIZE);
        let resp_dma = Arc::new(DmaStream::alloc(resp_frames, false).unwrap());
        let resp_slice = Slice::new(resp_dma, 0..resp_len);
        resp_slice
            .write_val(0, &VirtioGpuCtrlHdr::default())
            .unwrap();
        resp_slice.sync_to_device().unwrap();

        self.submit_control_dma_buffers(&[&req_slice], &[&resp_slice])?;

        resp_slice.sync_from_device().unwrap();
        let resp_hdr: VirtioGpuCtrlHdr = resp_slice.read_val(0).unwrap();
        if resp_hdr.type_ != RESP_OK_NODATA {
            return Err(VirtioGpuCommandError::UnexpectedResponse(resp_hdr.type_));
        }
        if resp_hdr.fence_id != fence_id {
            return Err(VirtioGpuCommandError::FenceMismatch {
                expected: fence_id,
                got: resp_hdr.fence_id,
            });
        }

        Ok(resp_hdr)
    }

    pub fn resource_flush(&self, resource_id: u32, rect: VirtioGpuRect) -> Result<(), VirtioGpuCommandError> {
        let req = VirtioGpuResourceFlush {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_RESOURCE_FLUSH,
                ..Default::default()
            },
            rect,
            resource_id,
            _padding: 0,
        };
        let _: VirtioGpuCtrlHdr = self
            .submit_control_command::<VirtioGpuResourceFlush, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(())
    }

    pub fn transfer_to_host_2d(
        &self,
        resource_id: u32,
        rect: VirtioGpuRect,
        offset: u64,
    ) -> Result<(), VirtioGpuCommandError> {
        let req = VirtioGpuTransferToHost2d {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_TRANSFER_TO_HOST_2D,
                ..Default::default()
            },
            rect,
            offset,
            resource_id,
            _padding: 0,
        };
        let _: VirtioGpuCtrlHdr = self
            .submit_control_command::<VirtioGpuTransferToHost2d, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(())
    }

    pub fn transfer_from_host_3d(
        &self,
        ctx_id: u32,
        resource_id: u32,
        box_: super::VirtioGpuBox,
        level: u32,
        offset: u64,
        stride: u32,
        layer_stride: u32,
    ) -> Result<u64, VirtioGpuCommandError> {
        let req = VirtioGpuTransferHost3d {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_TRANSFER_FROM_HOST_3D,
                ctx_id,
                ..Default::default()
            },
            box_,
            offset,
            resource_id,
            level,
            stride,
            layer_stride,
        };
        let (_resp, fence_id) = self
            .submit_control_command_with_fence::<VirtioGpuTransferHost3d, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(fence_id)
    }

    pub fn transfer_to_host_3d(
        &self,
        ctx_id: u32,
        resource_id: u32,
        box_: super::VirtioGpuBox,
        level: u32,
        offset: u64,
        stride: u32,
        layer_stride: u32,
    ) -> Result<u64, VirtioGpuCommandError> {
        let req = VirtioGpuTransferHost3d {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_TRANSFER_TO_HOST_3D,
                ctx_id,
                ..Default::default()
            },
            box_,
            offset,
            resource_id,
            level,
            stride,
            layer_stride,
        };
        let (_resp, fence_id) = self
            .submit_control_command_with_fence::<VirtioGpuTransferHost3d, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(fence_id)
    }

    pub fn set_scanout(
        &self,
        scanout_id: u32,
        resource_id: u32,
        rect: VirtioGpuRect,
    ) -> Result<(), VirtioGpuCommandError> {
        let req = VirtioGpuSetScanout {
            hdr: VirtioGpuCtrlHdr {
                type_: CMD_SET_SCANOUT,
                ..Default::default()
            },
            rect,
            scanout_id,
            resource_id,
        };
        let _: VirtioGpuCtrlHdr =
            self.submit_control_command::<VirtioGpuSetScanout, VirtioGpuCtrlHdr>(
                &req,
                RESP_OK_NODATA,
            )?;
        Ok(())
    }

    pub fn submit_3d(&self, command: &[u8], ctx_id: u32, ring_idx: Option<u8>) -> Result<u64, VirtioGpuCommandError> {
        if command.is_empty() {
            return Err(VirtioGpuCommandError::InvalidParameter);
        }

        let size = u32::try_from(command.len()).map_err(|_| VirtioGpuCommandError::InvalidParameter)?;
        let mut hdr = VirtioGpuCtrlHdr {
            type_: CMD_SUBMIT_3D,
            ctx_id,
            ..Default::default()
        };
        if let Some(idx) = ring_idx {
            hdr.flags |= VIRTIO_GPU_FLAG_INFO_RING_IDX;
            hdr.ring_idx = idx;
        }
        let req = VirtioGpuCmdSubmit {
            hdr,
            size,
            padding: 0,
        };

        let req_len = size_of::<VirtioGpuCmdSubmit>();
        let req_dma = Arc::new(DmaStream::alloc(req_len.div_ceil(PAGE_SIZE), false).unwrap());
        let req_slice = Slice::new(req_dma, 0..req_len);
        req_slice.write_val(0, &req).unwrap();
        let fence_id = self.alloc_fence_id();
        self.stamp_fence(&req_slice, fence_id);

        let cmd_dma = Arc::new(DmaStream::alloc(command.len().div_ceil(PAGE_SIZE), false).unwrap());
        let cmd_slice = Slice::new(cmd_dma, 0..command.len());
        cmd_slice.write_bytes(0, command).unwrap();

        let resp_len = size_of::<VirtioGpuCtrlHdr>();
        let resp_dma = Arc::new(DmaStream::alloc(resp_len.div_ceil(PAGE_SIZE), false).unwrap());
        let resp_slice = Slice::new(resp_dma, 0..resp_len);
        resp_slice.write_val(0, &VirtioGpuCtrlHdr::default()).unwrap();
        resp_slice.sync_to_device().unwrap();

        self.submit_control_dma_buffers(&[&req_slice, &cmd_slice], &[&resp_slice])?;

        resp_slice.sync_from_device().unwrap();
        let resp_hdr: VirtioGpuCtrlHdr = resp_slice.read_val(0).unwrap();
        if resp_hdr.type_ != RESP_OK_NODATA {
            return Err(VirtioGpuCommandError::UnexpectedResponse(resp_hdr.type_));
        }
        if resp_hdr.fence_id != fence_id {
            return Err(VirtioGpuCommandError::FenceMismatch {
                expected: fence_id,
                got: resp_hdr.fence_id,
            });
        }

        Ok(fence_id)
    }

    pub fn num_queues(&self) -> usize {
        2
    }

    pub fn num_capsets(&self) -> u32 {
        *self.num_capsets.lock()
    }

    pub fn num_scanouts(&self) -> u32 {
        *self.num_scanouts.lock()
    }

    pub fn has_virgl_3d(&self) -> bool {
        self.caps.contains(VirtioGpuCaps::VIRGL_3D)
    }

    pub fn has_edid(&self) -> bool {
        self.caps.contains(VirtioGpuCaps::EDID)
    }

    pub fn has_indirect(&self) -> bool {
        self.caps.contains(VirtioGpuCaps::INDIRECT_DESC)
    }

    pub fn has_resource_assign_uuid(&self) -> bool {
        self.caps.contains(VirtioGpuCaps::RESOURCE_ASSIGN_UUID)
    }

    pub fn has_resource_blob(&self) -> bool {
        self.caps.contains(VirtioGpuCaps::RESOURCE_BLOB)
    }

    pub fn has_context_init(&self) -> bool {
        self.caps.contains(VirtioGpuCaps::CONTEXT_INIT)
    }

    pub fn display_infos(&self) -> Vec<VirtioGpuDisplayOne> {
        self.display_infos.lock().clone()
    }

    pub fn edids(&self) -> Vec<Option<VirtioGpuRespEdid>> {
        self.edids.lock().clone()
    }

    pub fn display_info_resp(&self) -> Option<VirtioGpuRespDisplayInfo> {
        *self.display_info_resp.lock()
    }

    pub fn capset_infos(&self) -> Vec<VirtioGpuRespCapsetInfo> {
        self.capset_infos.lock().clone()
    }

    pub(crate) fn submit_control_command<Req, Resp>(
        &self,
        req: &Req,
        expected_resp_type: u32,
    ) -> Result<Resp, VirtioGpuCommandError>
    where
        Req: Pod,
        Resp: Pod + Default,
    {
        let (resp, _fence_id) = self.submit_control_command_with_fence(req, expected_resp_type)?;
        Ok(resp)
    }

    pub(crate) fn submit_control_command_with_fence<Req, Resp>(
        &self,
        req: &Req,
        expected_resp_type: u32,
    ) -> Result<(Resp, u64), VirtioGpuCommandError>
    where
        Req: Pod,
        Resp: Pod + Default,
    {
        if size_of::<Req>() > CTRL_REQ_STRIDE {
            println!(
                "[virtio-gpu] error: control command request size {} exceeds the stride {}",
                size_of::<Req>(),
                CTRL_REQ_STRIDE
            );
            return Err(VirtioGpuCommandError::RequestTooLarge(size_of::<Req>()));
        }
        if size_of::<Resp>() > CTRL_RESP_STRIDE {
            println!(
                "[virtio-gpu] error: control command response size {} exceeds the stride {}",
                size_of::<Resp>(),
                CTRL_RESP_STRIDE
            );
            return Err(VirtioGpuCommandError::ResponseTooLarge(size_of::<Resp>()));
        }

        let id = self.id_allocator.alloc();

        let req_slice = {
            let req_slice = Slice::new(
                self.ctrl_requests.clone(),
                id * CTRL_REQ_STRIDE..(id + 1) * CTRL_REQ_STRIDE,
            );
            req_slice.write_val(0, req).unwrap();
            let fence_id = self.alloc_fence_id();
            self.stamp_fence(&req_slice, fence_id);
            req_slice
        };

        let resp_slice = {
            let resp_slice = Slice::new(
                self.ctrl_responses.clone(),
                id * CTRL_RESP_STRIDE..(id + 1) * CTRL_RESP_STRIDE,
            );
            let default_resp = Resp::default();
            resp_slice.write_val(0, &default_resp).unwrap();
            resp_slice.sync_to_device().unwrap();
            resp_slice
        };

        let req_fence_id: u64 = req_slice.read_val::<VirtioGpuCtrlHdr>(0).unwrap().fence_id;

        self.submit_control_dma_buffers(&[&req_slice], &[&resp_slice])?;

        self.id_allocator.dealloc(id);

        resp_slice.sync_from_device().unwrap();
        let resp_hdr: VirtioGpuCtrlHdr = resp_slice.read_val(0).unwrap();
        if resp_hdr.type_ != expected_resp_type && resp_hdr.type_ != RESP_OK_NODATA {
            return Err(VirtioGpuCommandError::UnexpectedResponse(resp_hdr.type_));
        }
        if resp_hdr.fence_id != req_fence_id {
            return Err(VirtioGpuCommandError::FenceMismatch {
                expected: req_fence_id,
                got: resp_hdr.fence_id,
            });
        }

        let resp: Resp = resp_slice.read_val(0).unwrap();
        Ok((resp, req_fence_id))
    }

    pub fn has_cursor_queue(&self) -> bool {
        let queue = self.cursor_queue.lock();
        queue.size() > 0
    }
}

#[derive(Debug)]
pub enum VirtioGpuCommandError {
    Queue(QueueError),
    InvalidParameter,
    RequestTooLarge(usize),
    ResponseTooLarge(usize),
    UnexpectedResponse(u32),
    FenceMismatch { expected: u64, got: u64 },
}

#[derive(Debug, Clone, Copy, Default, Pod)]
#[repr(C)]
struct VirtioGpuResourceAttachBackingHdr {
    hdr: VirtioGpuCtrlHdr,
    resource_id: u32,
    nr_entries: u32,
}
