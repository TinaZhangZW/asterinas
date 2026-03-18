use alloc::{boxed::Box, format, sync::Arc, vec::Vec};
use core::cmp::min;

use aster_gpu::drm::{
    DrmError,
    device::DrmDevice,
    ioctl::DrmModeCrtc,
    mode_config::{
        DrmModeConfig, DrmModeModeInfo,
        connector::{
            ConnectorStatus, DrmConnector, DrmModeConnType,
            funcs::{ConnectorFuncs, drm_helper_probe_single_connector_modes},
        },
        crtc::{DrmCrtc, funcs::CrtcFuncs, helper::drm_atomic_helper_page_flip},
        encoder::{DrmEncoder, EncoderType, funcs::EncoderFuncs},
        framebuffer::DrmFramebuffer,
        plane::{DrmPlane, PlaneType, funcs::PlaneFuncs},
    },
    vblank::DrmPendingVblankEvent,
};
use crate::device::gpu::device::VirtioGpuDevice;
use crate::device::gpu::VirtioGpuRect;

pub fn virtio_gpu_output_init(
    scanout: u32,
    mode_config: &mut DrmModeConfig,
    vgpu: Arc<VirtioGpuDevice>,
) -> Result<(), DrmError> {
    let primary = DrmPlane::init(mode_config, PlaneType::Primary, Box::new(VirtioPlaneFuncs))?;
    let cursor = DrmPlane::init(mode_config, PlaneType::Cursor, Box::new(VirtioPlaneFuncs))?;

    let crtc = DrmCrtc::init_with_planes(
        mode_config,
        None,
        primary,
        Some(cursor),
        Box::new(VirtioCrtcFuncs { vgpu: vgpu.clone() }),
    )?;

    let encoder = DrmEncoder::init_with_crtcs(
        mode_config,
        EncoderType::VIRTUAL,
        &[crtc.clone()],
        Box::new(VirtioEncoderFuncs),
    )?;

    let mut connector = DrmConnector::init_with_encoder(
        mode_config,
        &[encoder],
        Box::new(VirtioConnectorFuncs {
            vgpu: vgpu.clone(),
            scanout,
        }),
    )?;
    if let Some(connector_mut) = Arc::get_mut(&mut connector) {
        connector_mut.set_connector_identity(DrmModeConnType::VIRTUAL, scanout + 1);

        // Linux virtio-gpu attaches the EDID property only when the feature is
        // advertised by the device. Start with value 0 and update it if an
        // EDID blob is available for this scanout.
        if vgpu.has_edid() {
            if let Some(prop_id) = mode_config.find_property_id_by_name("EDID") {
                connector_mut.attach_property(prop_id, 0);

                if let Some(edid) = vgpu.edids().get(scanout as usize).cloned().flatten() {
                    let size = min(edid.size as usize, edid.edid.len());
                    if size != 0 {
                        let blob_data =
                            Arc::<[u8]>::from(edid.edid[..size].to_vec().into_boxed_slice());
                        let blob_id = mode_config.create_blob(blob_data);
                        connector_mut.attach_property(prop_id, blob_id as u64);
                    }
                }
            }
        }
    }

    mode_config.register_connector(connector.clone());

    // Prime connector state/modes once during init, then userspace-triggered
    // fill_modes() will refresh through the same callback path.
    connector.funcs.detect(false, connector.clone())?;
    connector.funcs.get_modes(connector.clone())?;

    // Legacy userspace (e.g. kms-quads) expects at least one active CRTC/fb
    // routing to bootstrap. We expose a synthetic non-zero fb binding that is
    // replaced on the first real MODE_SETCRTC/atomic commit.
    if matches!(connector.status(), ConnectorStatus::Connected) {
        crtc.update_primary_plane_state(1);
    }

    Ok(())
}

#[derive(Debug)]
struct VirtioPlaneFuncs;

#[derive(Debug)]
struct VirtioCrtcFuncs {
    vgpu: Arc<VirtioGpuDevice>,
}

#[derive(Debug)]
struct VirtioConnectorFuncs {
    vgpu: Arc<VirtioGpuDevice>,
    scanout: u32,
}

#[derive(Debug)]
struct VirtioEncoderFuncs;

#[derive(Debug)]
struct ParsedEdid {
    preferred_mode: DrmModeModeInfo,
    mm_width: u32,
    mm_height: u32,
}

impl VirtioConnectorFuncs {
    fn scanout_info(&self) -> Option<crate::device::gpu::VirtioGpuDisplayOne> {
        self.vgpu.display_infos().get(self.scanout as usize).copied()
    }

    fn scanout_edid(&self) -> Option<crate::device::gpu::VirtioGpuRespEdid> {
        self.vgpu
            .edids()
            .get(self.scanout as usize)
            .cloned()
            .flatten()
    }
}

impl PlaneFuncs for VirtioPlaneFuncs {}

impl CrtcFuncs for VirtioCrtcFuncs {
    fn page_flip(
        &self,
        device: Arc<DrmDevice>,
        crtc: Arc<DrmCrtc>,
        fb: Arc<DrmFramebuffer>,
        event: Option<DrmPendingVblankEvent>,
        flags: u32,
        target: Option<u32>,
    ) -> Result<(), DrmError> {
        drm_atomic_helper_page_flip(device, crtc, fb, event, flags, target)
    }

    fn set_config(
        &self,
        crtc: Arc<DrmCrtc>,
        fb: Arc<DrmFramebuffer>,
        crtc_req: &DrmModeCrtc,
    ) -> Result<(), DrmError> {
        let gem_object = fb.gem_object();
        let resource_id = crate::device::gpu::drm::gem::virtio_gpu_obj_resource_id(&gem_object)?;
        let (guest_blob, host3d_blob) =
            crate::device::gpu::drm::gem::virtio_gpu_blob_state_by_gem(&gem_object)?;
        let is_dumb = crate::device::gpu::drm::gem::virtio_gpu_is_dumb_by_gem(&gem_object)?;
        let is_blob = guest_blob || host3d_blob;

        // Keep legacy modeset path simple and robust: scan out the whole FB.
        // Most userspace (including the double-buffer sample) uses x=y=0 and
        // mode size equal to the framebuffer size.
        let _ = crtc_req;
        let width = fb.width();
        let height = fb.height();

        let rect = VirtioGpuRect {
            x: 0,
            y: 0,
            width,
            height,
        };
        let scanout_id = crtc.index() as u32;

        if is_blob {
            // Linux uses SET_SCANOUT_BLOB for blob resources.
            // Our fb object is single-plane XRGB8888 today.
            self.vgpu
                .set_scanout_blob(
                    scanout_id,
                    resource_id,
                    rect,
                    width,
                    height,
                    crate::device::gpu::VirtioGpuFormat::B8G8R8X8Unorm as u32,
                    [fb.pitch(), 0, 0, 0],
                    [0, 0, 0, 0],
                )
                .map_err(|_| DrmError::Invalid)?;
        } else {
            // Linux only issues TRANSFER_TO_HOST_2D for dumb buffers.
            if is_dumb {
                let _ = self.vgpu.transfer_to_host_2d(resource_id, rect, 0);
            }
            self.vgpu
                .set_scanout(scanout_id, resource_id, rect)
                .map_err(|_| DrmError::Invalid)?;
        }
        // and finally flush to update the currently bound scanout
        self.vgpu
            .resource_flush(resource_id, rect)
            .map_err(|_| DrmError::Invalid)
    }

    fn enable_vblank(&self, _crtc: Arc<DrmCrtc>) -> Result<(), DrmError> {
        todo!()
    }

    fn disable_vblank(&self, _crtc: Arc<DrmCrtc>) -> Result<(), DrmError> {
        todo!()
    }
}

impl EncoderFuncs for VirtioEncoderFuncs {}

impl ConnectorFuncs for VirtioConnectorFuncs {
    fn fill_modes(
        &self,
        max_x: u32,
        max_y: u32,
        connector: Arc<DrmConnector>,
    ) -> Result<(), DrmError> {
        drm_helper_probe_single_connector_modes(max_x, max_y, connector)
    }

    fn detect(&self, _force: bool, connector: Arc<DrmConnector>) -> Result<(), DrmError> {
        let connected = self
            .scanout_info()
            .map(|info| info.enabled != 0)
            .unwrap_or(false);
        if connected {
            connector.update_status(ConnectorStatus::Connected)
        } else {
            connector.update_status(ConnectorStatus::Disconnected)
        }
    }

    fn get_modes(&self, connector: Arc<DrmConnector>) -> Result<(), DrmError> {
        let info = match self.scanout_info() {
            Some(info) => info,
            None => {
                connector.update_status(ConnectorStatus::Disconnected)?;
                connector.update_modes(&[])?;
                return Ok(());
            }
        };

        if info.enabled == 0 {
            connector.update_status(ConnectorStatus::Disconnected)?;
            connector.update_modes(&[])?;
            return Ok(());
        }

        connector.update_status(ConnectorStatus::Connected)?;

        let mut modes: Vec<DrmModeModeInfo> = Vec::new();

        // Linux virtio-gpu prefers EDID-derived modes and only falls back to
        // scanout geometry when EDID does not provide usable timings.
        if let Some(parsed_edid) = self.scanout_edid().and_then(parse_preferred_edid_mode) {
            connector.update_display_info(parsed_edid.mm_width, parsed_edid.mm_height, 0);
            modes.push(parsed_edid.preferred_mode);
        }

        if info.rect.width != 0 && info.rect.height != 0 {
            let fallback = mode_from_size(info.rect.width, info.rect.height);
            if !modes
                .iter()
                .any(|mode| mode.hdisplay as u32 == info.rect.width && mode.vdisplay as u32 == info.rect.height)
            {
                modes.push(fallback);
            }
        }

        if modes.is_empty() {
            modes.push(mode_from_size(1024, 768));
        }

        connector.update_modes(&modes)?;
        Ok(())
    }
}

fn parse_preferred_edid_mode(resp: crate::device::gpu::VirtioGpuRespEdid) -> Option<ParsedEdid> {
    let size = min(resp.size as usize, resp.edid.len());
    if size < 128 {
        return None;
    }
    let edid = &resp.edid[..size];
    if edid[0..8] != [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00] {
        return None;
    }

    // Detailed timing descriptors are in bytes 54..126. Use the first valid
    // one as the preferred timing for now.
    for dtd in (54..=108).step_by(18) {
        let pixclk_10khz = u16::from_le_bytes([edid[dtd], edid[dtd + 1]]) as u32;
        if pixclk_10khz == 0 {
            continue;
        }

        let hactive = (edid[dtd + 2] as u32) | (((edid[dtd + 4] as u32) & 0xF0) << 4);
        let hblank = (edid[dtd + 3] as u32) | (((edid[dtd + 4] as u32) & 0x0F) << 8);
        let vactive = (edid[dtd + 5] as u32) | (((edid[dtd + 7] as u32) & 0xF0) << 4);
        let vblank = (edid[dtd + 6] as u32) | (((edid[dtd + 7] as u32) & 0x0F) << 8);

        if hactive == 0 || vactive == 0 {
            continue;
        }

        let hsync_offset = (edid[dtd + 8] as u32) | (((edid[dtd + 11] as u32) & 0xC0) << 2);
        let hsync_pulse = (edid[dtd + 9] as u32) | (((edid[dtd + 11] as u32) & 0x30) << 4);
        let vsync_offset = (((edid[dtd + 10] as u32) >> 4) & 0x0F)
            | (((edid[dtd + 11] as u32) & 0x0C) << 2);
        let vsync_pulse = ((edid[dtd + 10] as u32) & 0x0F)
            | (((edid[dtd + 11] as u32) & 0x03) << 4);

        let hdisplay = hactive.min(u16::MAX as u32) as u16;
        let vdisplay = vactive.min(u16::MAX as u32) as u16;
        let htotal = (hactive + hblank).min(u16::MAX as u32) as u16;
        let vtotal = (vactive + vblank).min(u16::MAX as u32) as u16;
        let hsync_start = (hactive + hsync_offset).min(u16::MAX as u32) as u16;
        let hsync_end = (hactive + hsync_offset + hsync_pulse).min(u16::MAX as u32) as u16;
        let vsync_start = (vactive + vsync_offset).min(u16::MAX as u32) as u16;
        let vsync_end = (vactive + vsync_offset + vsync_pulse).min(u16::MAX as u32) as u16;

        let clock = pixclk_10khz.saturating_mul(10);
        let vrefresh = if htotal != 0 && vtotal != 0 {
            ((clock.saturating_mul(1000)) / ((htotal as u32).saturating_mul(vtotal as u32))).max(1)
        } else {
            60
        };

        let mut name = [0u8; 32];
        let s = format!("{}x{}", hactive, vactive);
        let bytes = s.as_bytes();
        let len = bytes.len().min(32);
        name[..len].copy_from_slice(&bytes[..len]);

        let mode = DrmModeModeInfo {
            clock,
            hdisplay,
            hsync_start,
            hsync_end,
            htotal,
            hskew: 0,
            vdisplay,
            vsync_start,
            vsync_end,
            vtotal,
            vscan: 0,
            vrefresh,
            // Keep sync flags conservative for now; many userspace stacks only
            // require valid geometry/clock for basic modesetting.
            flags: 0,
            type_: 0x60,
            name,
        };

        return Some(ParsedEdid {
            preferred_mode: mode,
            mm_width: edid[21] as u32,
            mm_height: edid[22] as u32,
        });
    }

    None
}

// TODO: dirty
fn mode_from_size(width: u32, height: u32) -> DrmModeModeInfo {
    let hdisplay = width.min(u16::MAX as u32) as u16;
    let vdisplay = height.min(u16::MAX as u32) as u16;

    let hsync_start = hdisplay.saturating_add(48);
    let hsync_end = hdisplay.saturating_add(80);
    let htotal = hdisplay.saturating_add(160);

    let vsync_start = vdisplay.saturating_add(3);
    let vsync_end = vdisplay.saturating_add(6);
    let vtotal = vdisplay.saturating_add(28);

    let vrefresh: u32 = 60;
    let clock = (htotal as u32) * (vtotal as u32) * vrefresh / 1000;

    let mut name = [0u8; 32];
    let s = format!("{width}x{height}");
    let bytes = s.as_bytes();
    let len = bytes.len().min(32);
    name[..len].copy_from_slice(&bytes[..len]);

    DrmModeModeInfo {
        clock,

        hdisplay,
        hsync_start,
        hsync_end,
        htotal,
        hskew: 0,

        vdisplay,
        vsync_start,
        vsync_end,
        vtotal,
        vscan: 0,

        vrefresh,

        flags: 0x5,  // DRM_MODE_FLAG_PHSYNC | DRM_MODE_FLAG_PVSYNC
        type_: 0x60, // DRM_MODE_TYPE_DRIVER | DRM_MODE_TYPE_PREFERRED

        name,
    }
}
