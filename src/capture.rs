use anyhow::{anyhow, Context, Result};
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, POINT, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_RESOURCE_MISC_FLAG, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIOutput, IDXGIOutput1, IDXGIOutputDuplication,
    IDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO, DXGI_OUTDUPL_POINTER_SHAPE_INFO,
};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};
use crate::pointer::PointerShape;

#[derive(Clone, Debug)]
pub struct DisplayInfo {
    pub adapter_index: u32,
    pub output_index: u32,
    pub name: String,
    pub bounds: RECT,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
    pub attached: bool,
    pub hmonitor: isize,
    pub adapter_luid: (i32, u32),
}

#[derive(Debug)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Vec<u8>,
    pub frame_id: u64,
}

pub fn enumerate_displays() -> Result<Vec<DisplayInfo>> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
    let mut displays = Vec::new();
    let mut adapter_index = 0;
    loop {
        let adapter: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(adapter_index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(error).context("DXGI enumeration failed"),
        };
        let adapter_desc = unsafe { adapter.GetDesc1()? };
        tracing::info!(adapter_index, description = %wide_string(&adapter_desc.Description), luid_high = adapter_desc.AdapterLuid.HighPart, luid_low = adapter_desc.AdapterLuid.LowPart, "DXGI adapter enumerated");
        let mut output_index = 0;
        loop {
            let output: IDXGIOutput = match unsafe { adapter.EnumOutputs(output_index) } {
                Ok(output) => output,
                Err(error) if error.code() == windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(error).context("DXGI enumeration failed"),
            };
            let desc = unsafe { output.GetDesc()? };
            let mut monitor_info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let is_primary = unsafe { GetMonitorInfoW(desc.Monitor, &mut monitor_info).as_bool() && monitor_info.dwFlags & 1 != 0 };
            let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).unsigned_abs();
            let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).unsigned_abs();
            tracing::info!(adapter_index, output_index, name = %wide_string(&desc.DeviceName), width, height, is_primary, attached = desc.AttachedToDesktop.as_bool(), hmonitor = desc.Monitor.0 as isize, left = desc.DesktopCoordinates.left, top = desc.DesktopCoordinates.top, right = desc.DesktopCoordinates.right, bottom = desc.DesktopCoordinates.bottom, "DXGI output detected");
            displays.push(DisplayInfo {
                adapter_index,
                output_index,
                name: wide_string(&desc.DeviceName),
                bounds: desc.DesktopCoordinates,
                width,
                height,
                is_primary,
                attached: desc.AttachedToDesktop.as_bool(),
                hmonitor: desc.Monitor.0 as isize,
                adapter_luid: (adapter_desc.AdapterLuid.HighPart, adapter_desc.AdapterLuid.LowPart),
            });
            output_index += 1;
        }
        adapter_index += 1;
    }
    Ok(displays)
}

pub fn select_parsec_display<'a>(displays: &'a [DisplayInfo], target: &parsec_vdd_rust::display::ParsecDisplay) -> Option<&'a DisplayInfo> {
    if !target.active || target.clone_of > 0 { return None; }
    let hmonitor = target.get_hmonitor_id()?;
    let mut matches = displays.iter().filter(|d| d.attached && d.width > 0 && d.height > 0
        && d.hmonitor == hmonitor && d.name.eq_ignore_ascii_case(&target.device_name));
    let selected = matches.next()?;
    if matches.next().is_some() { return None; }
    Some(selected)
}

pub struct DesktopDuplicator {
    // Rust drops fields in declaration order: release children before the device.
    duplication: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    frame_id: u64,
    last_frame: Instant,
    timeouts: u64,
    pointer_shape: Option<PointerShape>,
    pointer_position: POINT,
    pointer_visible: bool,
}

fn dxgi_failure(operation: &str, error: windows::core::Error) -> anyhow::Error {
    let code = error.code();
    let class = match code.0 as u32 {
        0x887A0026 => "DXGI_ERROR_ACCESS_LOST",
        0x887A0027 => "DXGI_ERROR_WAIT_TIMEOUT",
        0x887A0028 => "DXGI_ERROR_SESSION_DISCONNECTED",
        0x887A0005 => "DXGI_ERROR_DEVICE_REMOVED",
        0x887A0007 => "DXGI_ERROR_DEVICE_RESET",
        0x887A0001 => "DXGI_ERROR_INVALID_CALL",
        0x887A0022 => "DXGI_ERROR_NOT_CURRENTLY_AVAILABLE",
        0x887A0004 => "DXGI_ERROR_UNSUPPORTED",
        0x80070005 => "E_ACCESSDENIED",
        0x80070057 => "E_INVALIDARG",
        _ => "OTHER",
    };
    tracing::warn!(operation, class, hresult = format_args!("0x{:08X}", code.0 as u32), %error, "DXGI call failed");
    anyhow::Error::new(error).context(format!("{operation}: {class} (0x{:08X})", code.0 as u32))
}

impl DesktopDuplicator {
    pub fn new(target: &DisplayInfo) -> Result<Self> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
        let adapter: IDXGIAdapter1 = unsafe { factory.EnumAdapters1(target.adapter_index)? };
        let output: IDXGIOutput = unsafe { adapter.EnumOutputs(target.output_index)? };
        let desc = unsafe { output.GetDesc()? };
        let adapter_desc = unsafe { adapter.GetDesc1()? };
        if wide_string(&desc.DeviceName) != target.name || desc.Monitor.0 as isize != target.hmonitor
            || !desc.AttachedToDesktop.as_bool() || desc.DesktopCoordinates != target.bounds
            || (adapter_desc.AdapterLuid.HighPart, adapter_desc.AdapterLuid.LowPart) != target.adapter_luid {
            return Err(anyhow!("DXGI topology changed during duplicator creation; re-enumerate"));
        }
        let output1: IDXGIOutput1 = output.cast()?;
        tracing::info!(name = %target.name, adapter_index = target.adapter_index, output_index = target.output_index, luid = ?target.adapter_luid, hmonitor = target.hmonitor, primary = target.is_primary, "creating desktop duplication");

        let mut device = None;
        let mut context = None;
        let adapter_base = adapter.cast()?;
        unsafe {
            D3D11CreateDevice(
            Some(&adapter_base),
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            ).map_err(|error| dxgi_failure("D3D11CreateDevice", error))?
        };
        let device = device.context("D3D11 device was not created")?;
        let context = context.context("D3D11 context was not created")?;
        let duplication = unsafe { output1.DuplicateOutput(&device).map_err(|error| dxgi_failure("DuplicateOutput", error))? };
        Ok(Self { device, context, duplication, staging: None, frame_id: 0, last_frame: Instant::now(), timeouts: 0,
            pointer_shape: None, pointer_position: POINT::default(), pointer_visible: false })
    }

    pub fn next_frame(&mut self, timeout: Duration) -> Result<Option<CapturedFrame>> {
        let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        let acquired = unsafe { self.duplication.AcquireNextFrame(timeout_ms, &mut info, &mut resource) };
        if let Err(error) = acquired {
            if error.code() == DXGI_ERROR_WAIT_TIMEOUT {
                self.timeouts += 1;
                if self.timeouts == 1 || self.timeouts.is_multiple_of(120) {
                    tracing::debug!(hresult = "0x887A0027", timeouts = self.timeouts, elapsed_ms = self.last_frame.elapsed().as_millis() as u64, "AcquireNextFrame: DXGI_ERROR_WAIT_TIMEOUT");
                }
                if self.last_frame.elapsed() >= Duration::from_secs(5) {
                    return Err(dxgi_failure("AcquireNextFrame: continuous timeout watchdog (static desktop is also possible)", error));
                }
                return Ok(None);
            }
            return Err(dxgi_failure("AcquireNextFrame", error));
        }

        // Always release an acquired frame, including missing-resource/copy errors.
        let result = (|| -> Result<CapturedFrame> {
            self.update_pointer(&info)?;
            let mut frame = self.copy_frame(resource.context("desktop duplication returned no resource")?)?;
            // The resource is copied afresh even on pointer-only updates, so the
            // previous cursor is never burned into the next frame.
            if self.pointer_visible {
                if let Some(shape) = &self.pointer_shape {
                    shape.composite(&mut frame, self.pointer_position);
                }
            }
            Ok(frame)
        })();
        let released = unsafe { self.duplication.ReleaseFrame() }.map_err(|error| dxgi_failure("ReleaseFrame", error));
        released?;
        if result.is_ok() {
            if self.frame_id == 1 || self.timeouts > 120 {
                tracing::info!(frame_id = self.frame_id, timeouts = self.timeouts, "AcquireNextFrame succeeded; capture receiving frames");
            }
            self.last_frame = Instant::now();
            self.timeouts = 0;
        }
        result.map(Some)
    }

    fn update_pointer(&mut self, info: &DXGI_OUTDUPL_FRAME_INFO) -> Result<()> {
        // Zero means no position/visibility update: retain the previous state.
        // Visible=false also covers cursors already baked into the desktop.
        if info.LastMouseUpdateTime != 0 {
            let visible = info.PointerPosition.Visible.as_bool();
            if visible != self.pointer_visible {
                tracing::debug!(visible, x = info.PointerPosition.Position.x,
                    y = info.PointerPosition.Position.y, "DXGI separate pointer visibility changed");
            }
            self.pointer_position = info.PointerPosition.Position;
            self.pointer_visible = visible;
        }
        if info.PointerShapeBufferSize != 0 {
            let mut bytes = vec![0; info.PointerShapeBufferSize as usize];
            let mut required = 0;
            let mut shape_info = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
            unsafe {
                self.duplication.GetFramePointerShape(bytes.len() as u32,
                    bytes.as_mut_ptr().cast(), &mut required, &mut shape_info)
            }.map_err(|error| dxgi_failure("GetFramePointerShape", error))?;
            tracing::debug!(kind = shape_info.Type, width = shape_info.Width,
                height = shape_info.Height, pitch = shape_info.Pitch,
                "DXGI pointer shape updated; compositing into captured frames");
            self.pointer_shape = Some(PointerShape::new(shape_info, bytes)?);
        }
        Ok(())
    }

    fn copy_frame(&mut self, resource: IDXGIResource) -> Result<CapturedFrame> {
        let texture: ID3D11Texture2D = resource.cast()?;
        let mut source_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut source_desc) };
        if source_desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            return Err(anyhow!("unsupported desktop format: {:?}", source_desc.Format));
        }
        let recreate = self.staging.as_ref().is_none_or(|staging| {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { staging.GetDesc(&mut desc) };
            desc.Width != source_desc.Width || desc.Height != source_desc.Height
        });
        if recreate {
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width: source_desc.Width,
                Height: source_desc.Height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
            };
            let mut staging = None;
            unsafe { self.device.CreateTexture2D(&staging_desc, None, Some(&mut staging))? };
            self.staging = Some(staging.context("failed to create staging texture")?);
        }
        let staging = self.staging.as_ref().context("staging texture missing")?;
        unsafe { self.context.CopyResource(staging, &texture) };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { self.context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).map_err(|error| dxgi_failure("Map staging texture", error))? };
        let stride = mapped.RowPitch;
        let row_len = source_desc.Width as usize * 4;
        let mut pixels = vec![0; row_len * source_desc.Height as usize];
        for row in 0..source_desc.Height as usize {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (mapped.pData as *const u8).add(row * stride as usize),
                    pixels.as_mut_ptr().add(row * row_len),
                    row_len,
                );
            }
        }
        unsafe { self.context.Unmap(staging, 0) };
        self.frame_id += 1;
        Ok(CapturedFrame { width: source_desc.Width, height: source_desc.Height, stride: row_len as u32, pixels, frame_id: self.frame_id })
    }
}

fn wide_string(value: &[u16]) -> String {
    let length = value.iter().position(|character| *character == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

use windows::Win32::Graphics::Dxgi::IDXGIResource;
