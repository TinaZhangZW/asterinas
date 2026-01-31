// SPDX-License-Identifier: MPL-2.0

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::mem::size_of;
use ostd::Pod;

use aster_gpu::{
    drm::{
        device::DrmDevice,
        driver::{DrmDriver, DrmDriverFeatures, DrmDriverOps, DrmUserCopyOut, DumbCreateProvider},
        gem::DrmGemObject,
        mode_config::{
            DrmModeConfig, DrmModeConfigFuncs, DrmModeModeInfo,
            connector::{ConnectorStatus, DrmConnector, funcs::ConnectorFuncs},
            crtc::{DrmCrtc, funcs::CrtcFuncs},
            encoder::{DrmEncoder, EncoderType, funcs::EncoderFuncs},
            plane::{DrmPlane, PlaneType, funcs::PlaneFuncs},
        },
    },
    drm_register_driver,
    register_driver,
};
use log::warn;

use super::{
    VIRTIO_GPU_DRIVER_DATE, VIRTIO_GPU_DRIVER_DESC, VIRTIO_GPU_DRIVER_NAME,
    device::{VirtioGpuDevice, VirtioGpuRect},
    virtgpu::{
        DrmVirtgpuContextInit, DrmVirtgpuExecbuffer, DrmVirtgpuGetCaps, DrmVirtgpuGetparam,
        DrmVirtgpuMap, DrmVirtgpuResourceCreate, DrmVirtgpuResourceCreateBlob,
        DrmVirtgpuResourceInfo, DrmVirtgpuResourceUnref, DrmVirtgpuTransfer, DrmVirtgpuWait,
        VIRTGPU_CONTEXT_INIT, VIRTGPU_EXECBUFFER, VIRTGPU_GET_CAPS, VIRTGPU_GETPARAM,
        VIRTGPU_MAP, VIRTGPU_RESOURCE_CREATE, VIRTGPU_RESOURCE_CREATE_BLOB,
        VIRTGPU_RESOURCE_INFO, VIRTGPU_RESOURCE_UNREF, VIRTGPU_TRANSFER_FROM_HOST,
        VIRTGPU_TRANSFER_TO_HOST, VIRTGPU_WAIT,
    },
};

drm_register_driver!(VirtioGpuDrmDriver, VIRTIO_GPU_DRIVER_NAME);

#[derive(Debug)]
struct VirtioModeConfigFuncs;

impl DrmModeConfigFuncs for VirtioModeConfigFuncs {
    fn fb_create(
        &self,
        mode_config: &mut DrmModeConfig,
        width: u32,
        height: u32,
        pitch: u32,
        bpp: u32,
        gem_obj: Arc<DrmGemObject>,
    ) -> u32 {
        mode_config.create_framebuffer_default(width, height, pitch, bpp, gem_obj)
    }
}

fn virtio_gpu_framebuffer_info(device: &DrmDevice) -> Option<aster_gpu::GpuFramebufferInfo> {
    let virtio = device.driver_data::<VirtioGpuDevice>()?;
    virtio.framebuffer_info_snapshot()
}

impl DrmDriver for VirtioGpuDrmDriver {
    fn name(&self) -> &str {
        VIRTIO_GPU_DRIVER_NAME
    }

    fn desc(&self) -> &str {
        VIRTIO_GPU_DRIVER_DESC
    }

    fn date(&self) -> &str {
        VIRTIO_GPU_DRIVER_DATE
    }

    fn create_device(&self, index: u32) -> Result<Arc<DrmDevice>, ()> {
        let (virtio_index, virtio) = match VirtioGpuDevice::device_by_index(index) {
            Some(virtio) => (index, virtio),
            None => {
                let fallback_index = index.checked_sub(1).ok_or(())?;
                let virtio = VirtioGpuDevice::device_by_index(fallback_index).ok_or(())?;
                (fallback_index, virtio)
            }
        };

        let dev = VirtioGpuDrmDevice::new(virtio_index, virtio);
        dev.init()?;
        Ok(dev.device.clone())
    }

    fn driver_features(&self) -> DrmDriverFeatures {
        DrmDriverFeatures::GEM | DrmDriverFeatures::MODESET
    }

    fn driver_ops(&self) -> DrmDriverOps {
        DrmDriverOps {
            dumb_create: Some(DumbCreateProvider::Memfd),
            ioctl: None,
            framebuffer_info: Some(virtio_gpu_framebuffer_info),
        }
    }

    fn alloc_command_buffer(&self, cmd: u32) -> Option<Vec<u8>> {
        let size = match cmd {
            VIRTGPU_MAP => size_of::<DrmVirtgpuMap>(),
            VIRTGPU_EXECBUFFER => size_of::<DrmVirtgpuExecbuffer>(),
            VIRTGPU_GETPARAM => size_of::<DrmVirtgpuGetparam>(),
            VIRTGPU_RESOURCE_CREATE => size_of::<DrmVirtgpuResourceCreate>(),
            VIRTGPU_RESOURCE_INFO => size_of::<DrmVirtgpuResourceInfo>(),
            VIRTGPU_RESOURCE_UNREF => size_of::<DrmVirtgpuResourceUnref>(),
            VIRTGPU_TRANSFER_FROM_HOST | VIRTGPU_TRANSFER_TO_HOST => size_of::<DrmVirtgpuTransfer>(),
            VIRTGPU_WAIT => size_of::<DrmVirtgpuWait>(),
            VIRTGPU_GET_CAPS => size_of::<DrmVirtgpuGetCaps>(),
            VIRTGPU_RESOURCE_CREATE_BLOB => size_of::<DrmVirtgpuResourceCreateBlob>(),
            VIRTGPU_CONTEXT_INIT => size_of::<DrmVirtgpuContextInit>(),
            _ => return None,
        };
        Some(alloc::vec![0u8; size])
    }

    fn handle_command(&self, device: &DrmDevice, cmd: u32, data: &mut [u8]) -> Result<(), ()> {
        let virtio = device.driver_data::<VirtioGpuDevice>().ok_or(())?;
        virtio.ioctl(cmd, data)
    }

    fn command_copyouts(&self, cmd: u32, data: &[u8]) -> Vec<DrmUserCopyOut> {
        if cmd != VIRTGPU_GET_CAPS || data.len() < size_of::<DrmVirtgpuGetCaps>() {
            return Vec::new();
        }

        let getcaps: DrmVirtgpuGetCaps =
            Pod::from_bytes(&data[..size_of::<DrmVirtgpuGetCaps>()]);
        if getcaps.caps == 0 || getcaps.size == 0 {
            return Vec::new();
        }

        let size = getcaps.size as usize;
        let zero = alloc::vec![0u8; size];
        vec![DrmUserCopyOut {
            user_ptr: getcaps.caps as usize,
            data: zero,
        }]
    }
}

/// Registers the virtio-gpu DRM driver in the global GPU registry.
pub fn register() {
    let driver = Arc::new(VirtioGpuDrmDriver {});
    register_driver(VIRTIO_GPU_DRIVER_NAME, driver)
        .expect("failed to register virtio-gpu DrmDriver");
}

#[derive(Debug)]
struct VirtioGpuDrmDevice {
    device: Arc<DrmDevice>,
    virtio: Arc<VirtioGpuDevice>,
}

impl VirtioGpuDrmDevice {
    fn new(index: u32, virtio: Arc<VirtioGpuDevice>) -> Self {
        let driver = Arc::new(VirtioGpuDrmDriver {});
        let driver_features = driver.driver_features();
        let device = Arc::new(DrmDevice::new(index, driver, driver_features));
        device.set_driver_data(virtio.clone());
        Self { device, virtio }
    }

    fn init(&self) -> Result<(), ()> {
        let mut resources = self.device.resources().lock();
        resources.init_standard_properties();
        resources.set_mode_config_funcs(Some(Arc::new(VirtioModeConfigFuncs)));

        let primary_plane = DrmPlane::init(
            &mut resources,
            PlaneType::Primary,
            Box::new(VirtioPlaneFuncs),
        )?;
        let crtc = DrmCrtc::init_with_planes(
            &mut resources,
            None,
            primary_plane,
            None,
            Box::new(VirtioCrtcFuncs),
        )?;
        let encoder = DrmEncoder::init_with_crtcs(
            &mut resources,
            EncoderType::VIRTUAL,
            &[crtc],
            Box::new(VirtioEncoderFuncs),
        )?;

        let modeinfo = match self.virtio.preferred_scanout() {
            Ok(Some((scanout_id, rect))) => {
                if let Err(err) = self.virtio.setup_scanout(scanout_id, rect) {
                    warn!("virtio-gpu scanout setup failed: {:?}", err);
                }
                modeinfo_from_rect(rect)
            }
            Ok(None) => {
                let rect = VirtioGpuRect {
                    x: 0,
                    y: 0,
                    width: 1024,
                    height: 768,
                };
                if let Err(err) = self.virtio.setup_scanout(0, rect) {
                    warn!("virtio-gpu default scanout setup failed: {:?}", err);
                }
                modeinfo_from_rect(rect)
            }
            Err(err) => {
                warn!("virtio-gpu display info query failed: {:?}", err);
                let rect = VirtioGpuRect {
                    x: 0,
                    y: 0,
                    width: 1024,
                    height: 768,
                };
                if let Err(err) = self.virtio.setup_scanout(0, rect) {
                    warn!("virtio-gpu default scanout setup failed: {:?}", err);
                }
                modeinfo_from_rect(rect)
            }
        };
        let _connector = DrmConnector::init_with_encoder(
            &mut resources,
            ConnectorStatus::Connected,
            &[modeinfo],
            &[encoder],
            Box::new(VirtioConnectorFuncs),
        )?;

        Ok(())
    }
}

#[derive(Debug)]
struct VirtioPlaneFuncs;
#[derive(Debug)]
struct VirtioCrtcFuncs;
#[derive(Debug)]
struct VirtioEncoderFuncs;
#[derive(Debug)]
struct VirtioConnectorFuncs;

impl PlaneFuncs for VirtioPlaneFuncs {}
impl CrtcFuncs for VirtioCrtcFuncs {}
impl EncoderFuncs for VirtioEncoderFuncs {}
impl ConnectorFuncs for VirtioConnectorFuncs {}

#[expect(dead_code)]
fn fake_modeinfo() -> DrmModeModeInfo {
    let mut name = [0u8; 32];
    let bytes = "1024x768".as_bytes();
    let len = bytes.len().min(32);
    name[..len].copy_from_slice(&bytes[..len]);

    DrmModeModeInfo {
        clock: 65000,
        hdisplay: 1024,
        hsync_start: 1048,
        hsync_end: 1184,
        htotal: 1344,
        hskew: 0,
        vdisplay: 768,
        vsync_start: 771,
        vsync_end: 777,
        vtotal: 806,
        vscan: 0,
        vrefresh: 60,
        flags: 0x5,
        type_: 0x40,
        name,
    }
}

fn modeinfo_from_rect(rect: VirtioGpuRect) -> DrmModeModeInfo {
    let width = rect.width.max(1);
    let height = rect.height.max(1);
    let mut name = [0u8; 32];
    let name_str = alloc::format!("{}x{}", width, height);
    let bytes = name_str.as_bytes();
    let len = bytes.len().min(32);
    name[..len].copy_from_slice(&bytes[..len]);

    let hsync_start = width + 48;
    let hsync_end = width + 80;
    let htotal = width + 160;
    let vsync_start = height + 3;
    let vsync_end = height + 6;
    let vtotal = height + 30;

    DrmModeModeInfo {
        clock: (htotal * vtotal * 60 / 1000).max(1),
        hdisplay: width as u16,
        hsync_start: hsync_start as u16,
        hsync_end: hsync_end as u16,
        htotal: htotal as u16,
        hskew: 0,
        vdisplay: height as u16,
        vsync_start: vsync_start as u16,
        vsync_end: vsync_end as u16,
        vtotal: vtotal as u16,
        vscan: 0,
        vrefresh: 60,
        flags: 0x5,
        type_: 0x40,
        name,
    }
}