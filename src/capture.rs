use crate::pointer::PointerShape;
use anyhow::{anyhow, Context, Result};
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, POINT, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_RESOURCE_MISC_FLAG, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IDXGIOutput1,
    IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
    DXGI_OUTDUPL_POINTER_SHAPE_INFO,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL, ICONINFO,
};

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
            Err(error) if error.code() == windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND => {
                break
            }
            Err(error) => return Err(error).context("DXGI enumeration failed"),
        };
        let adapter_desc = unsafe { adapter.GetDesc1()? };
        tracing::info!(adapter_index, description = %wide_string(&adapter_desc.Description), luid_high = adapter_desc.AdapterLuid.HighPart, luid_low = adapter_desc.AdapterLuid.LowPart, "DXGI adapter enumerated");
        let mut output_index = 0;
        loop {
            let output: IDXGIOutput = match unsafe { adapter.EnumOutputs(output_index) } {
                Ok(output) => output,
                Err(error)
                    if error.code() == windows::Win32::Graphics::Dxgi::DXGI_ERROR_NOT_FOUND =>
                {
                    break
                }
                Err(error) => return Err(error).context("DXGI enumeration failed"),
            };
            let desc = unsafe { output.GetDesc()? };
            let mut monitor_info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let is_primary = unsafe {
                GetMonitorInfoW(desc.Monitor, &mut monitor_info).as_bool()
                    && monitor_info.dwFlags & 1 != 0
            };
            let width =
                (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).unsigned_abs();
            let height =
                (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).unsigned_abs();
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
                adapter_luid: (
                    adapter_desc.AdapterLuid.HighPart,
                    adapter_desc.AdapterLuid.LowPart,
                ),
            });
            output_index += 1;
        }
        adapter_index += 1;
    }
    Ok(displays)
}

pub fn select_parsec_display<'a>(
    displays: &'a [DisplayInfo],
    target: &parsec_vdd_rust::display::ParsecDisplay,
) -> Option<&'a DisplayInfo> {
    if !target.active || target.clone_of > 0 {
        return None;
    }
    let hmonitor = target.get_hmonitor_id()?;
    let mut matches = displays.iter().filter(|d| {
        d.attached
            && d.width > 0
            && d.height > 0
            && d.hmonitor == hmonitor
            && d.name.eq_ignore_ascii_case(&target.device_name)
    });
    let selected = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(selected)
}

pub struct GdiCapturer {
    bounds: RECT,
    width: u32,
    height: u32,
    hdc_mem: HDC,
    hbitmap: HBITMAP,
    old_bmp: HGDIOBJ,
    bmi: BITMAPINFO,
    frame_id: u64,
}

impl GdiCapturer {
    pub fn new(target: &DisplayInfo) -> Result<Self> {
        let width = target.width;
        let height = target.height;
        if width == 0 || height == 0 {
            return Err(anyhow!(
                "Invalid display dimensions for GDI capture: {}x{}",
                width,
                height
            ));
        }

        let hdc_screen = unsafe { GetDC(None) };
        if hdc_screen.is_invalid() {
            return Err(anyhow!("Failed to acquire screen DC"));
        }

        let hdc_mem = unsafe { CreateCompatibleDC(Some(hdc_screen)) };
        if hdc_mem.is_invalid() {
            unsafe {
                let _ = ReleaseDC(None, hdc_screen);
            }
            return Err(anyhow!("Failed to create compatible memory DC"));
        }

        let hbitmap = unsafe { CreateCompatibleBitmap(hdc_screen, width as i32, height as i32) };
        if hbitmap.is_invalid() {
            unsafe {
                let _ = DeleteDC(hdc_mem);
                let _ = ReleaseDC(None, hdc_screen);
            }
            return Err(anyhow!("Failed to create compatible bitmap"));
        }

        let old_bmp = unsafe { SelectObject(hdc_mem, hbitmap.into()) };
        unsafe {
            let _ = ReleaseDC(None, hdc_screen);
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32), // negative for top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        tracing::info!(
            width,
            height,
            left = target.bounds.left,
            top = target.bounds.top,
            "GDI Capturer initialized successfully"
        );

        Ok(Self {
            bounds: target.bounds,
            width,
            height,
            hdc_mem,
            hbitmap,
            old_bmp,
            bmi,
            frame_id: 0,
        })
    }

    pub fn capture_frame(&mut self) -> Result<CapturedFrame> {
        let hdc_screen = unsafe { GetDC(None) };
        if hdc_screen.is_invalid() {
            return Err(anyhow!("Failed to acquire screen DC"));
        }

        let src_x = self.bounds.left;
        let src_y = self.bounds.top;
        let width = self.width as i32;
        let height = self.height as i32;

        let blt_res = unsafe {
            BitBlt(
                self.hdc_mem,
                0,
                0,
                width,
                height,
                Some(hdc_screen),
                src_x,
                src_y,
                SRCCOPY,
            )
        };
        unsafe {
            let _ = ReleaseDC(None, hdc_screen);
        }
        if let Err(e) = blt_res {
            return Err(anyhow!("GDI BitBlt failed: {e}"));
        }

        // Composite mouse cursor if inside virtual display bounds
        let mut ci = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetCursorInfo(&mut ci).is_ok() } && (ci.flags.0 & CURSOR_SHOWING.0 != 0) {
            let pt = ci.ptScreenPos;
            if pt.x >= self.bounds.left
                && pt.x < self.bounds.right
                && pt.y >= self.bounds.top
                && pt.y < self.bounds.bottom
            {
                let mut icon_info = ICONINFO::default();
                if unsafe { GetIconInfo(ci.hCursor.into(), &mut icon_info).is_ok() } {
                    let draw_x = (pt.x - self.bounds.left) - icon_info.xHotspot as i32;
                    let draw_y = (pt.y - self.bounds.top) - icon_info.yHotspot as i32;
                    unsafe {
                        let _ = DrawIconEx(
                            self.hdc_mem,
                            draw_x,
                            draw_y,
                            ci.hCursor.into(),
                            0,
                            0,
                            0,
                            None,
                            DI_NORMAL,
                        );
                        if !icon_info.hbmMask.is_invalid() {
                            let _ = DeleteObject(icon_info.hbmMask.into());
                        }
                        if !icon_info.hbmColor.is_invalid() {
                            let _ = DeleteObject(icon_info.hbmColor.into());
                        }
                    }
                }
            }
        }

        let row_len = (self.width * 4) as usize;
        let mut pixels = vec![0u8; row_len * self.height as usize];

        let lines = unsafe {
            GetDIBits(
                self.hdc_mem,
                self.hbitmap,
                0,
                self.height,
                Some(pixels.as_mut_ptr().cast()),
                &mut self.bmi,
                DIB_RGB_COLORS,
            )
        };
        if lines == 0 {
            return Err(anyhow!("GDI GetDIBits failed"));
        }

        // Force alpha = 255 (GDI BitBlt writes 0 in the alpha channel for RGB desktop)
        for chunk in pixels.chunks_exact_mut(4) {
            chunk[3] = 255;
        }

        self.frame_id += 1;
        Ok(CapturedFrame {
            width: self.width,
            height: self.height,
            stride: row_len as u32,
            pixels,
            frame_id: self.frame_id,
        })
    }
}

impl Drop for GdiCapturer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc_mem, self.old_bmp);
            let _ = DeleteObject(self.hbitmap.into());
            let _ = DeleteDC(self.hdc_mem);
        }
    }
}

struct DxgiState {
    duplication: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    pointer_shape: Option<PointerShape>,
    pointer_position: POINT,
    pointer_visible: bool,
}

pub struct DesktopDuplicator {
    dxgi: Option<DxgiState>,
    gdi: Option<GdiCapturer>,
    target: DisplayInfo,
    frame_id: u64,
    last_frame: Instant,
    timeouts: u64,
    dxgi_consecutive_timeouts: u64,
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
        let mut duplicator = Self {
            dxgi: None,
            gdi: None,
            target: target.clone(),
            frame_id: 0,
            last_frame: Instant::now(),
            timeouts: 0,
            dxgi_consecutive_timeouts: 0,
        };

        match duplicator.init_dxgi() {
            Ok(state) => {
                duplicator.dxgi = Some(state);
                tracing::info!("Desktop duplication initialized with DXGI backend");
            }
            Err(e) => {
                tracing::warn!(error = %e, "DXGI initialization failed, falling back to GDI capturer");
                duplicator.gdi = Some(GdiCapturer::new(target)?);
            }
        }

        Ok(duplicator)
    }

    fn init_dxgi(&self) -> Result<DxgiState> {
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1()? };
        let adapter: IDXGIAdapter1 = unsafe { factory.EnumAdapters1(self.target.adapter_index)? };
        let output: IDXGIOutput = unsafe { adapter.EnumOutputs(self.target.output_index)? };
        let desc = unsafe { output.GetDesc()? };
        let adapter_desc = unsafe { adapter.GetDesc1()? };
        if wide_string(&desc.DeviceName) != self.target.name
            || desc.Monitor.0 as isize != self.target.hmonitor
            || !desc.AttachedToDesktop.as_bool()
            || desc.DesktopCoordinates != self.target.bounds
            || (
                adapter_desc.AdapterLuid.HighPart,
                adapter_desc.AdapterLuid.LowPart,
            ) != self.target.adapter_luid
        {
            return Err(anyhow!(
                "DXGI topology changed during duplicator creation; re-enumerate"
            ));
        }
        let output1: IDXGIOutput1 = output.cast()?;
        tracing::info!(name = %self.target.name, adapter_index = self.target.adapter_index, output_index = self.target.output_index, luid = ?self.target.adapter_luid, hmonitor = self.target.hmonitor, primary = self.target.is_primary, "creating desktop duplication");

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
            )
            .map_err(|error| dxgi_failure("D3D11CreateDevice", error))?
        };
        let device = device.context("D3D11 device was not created")?;
        let context = context.context("D3D11 context was not created")?;
        let duplication = unsafe {
            output1
                .DuplicateOutput(&device)
                .map_err(|error| dxgi_failure("DuplicateOutput", error))?
        };
        Ok(DxgiState {
            device,
            context,
            duplication,
            staging: None,
            pointer_shape: None,
            pointer_position: POINT::default(),
            pointer_visible: false,
        })
    }

    pub fn next_frame(&mut self, timeout: Duration) -> Result<Option<CapturedFrame>> {
        // If already using GDI, capture frame with ~60fps rate-limit (16ms)
        if let Some(gdi) = &mut self.gdi {
            if self.frame_id > 0 && self.last_frame.elapsed() < Duration::from_millis(16) {
                return Ok(None);
            }
            let frame = gdi.capture_frame()?;
            self.frame_id = frame.frame_id;
            self.last_frame = Instant::now();
            self.timeouts = 0;
            return Ok(Some(frame));
        }

        // Otherwise try DXGI
        if let Some(mut dxgi) = self.dxgi.take() {
            match self.next_frame_dxgi(&mut dxgi, timeout) {
                Ok(Some(frame)) => {
                    self.dxgi = Some(dxgi);
                    self.dxgi_consecutive_timeouts = 0;
                    self.timeouts = 0;
                    self.last_frame = Instant::now();
                    self.frame_id = frame.frame_id;
                    return Ok(Some(frame));
                }
                Ok(None) => {
                    self.dxgi = Some(dxgi);
                    self.timeouts += 1;
                    self.dxgi_consecutive_timeouts += 1;
                    // If DXGI has never produced an initial frame within 1.0 second,
                    // DWM is blocking DXGI OutputDuplication (as observed on Windows 11 26H2 / Insider).
                    // Fall back to GDI capturer immediately so the display is never left black.
                    if self.frame_id == 0
                        && self.last_frame.elapsed() >= Duration::from_millis(1000)
                    {
                        tracing::warn!(
                            elapsed_ms = self.last_frame.elapsed().as_millis() as u64,
                            "DXGI OutputDuplication produced no initial frame within 1s (DWM blocking detected); switching to GDI capturer"
                        );
                        self.dxgi = None; // Drop DXGI resources
                        let mut gdi = GdiCapturer::new(&self.target)?;
                        let frame = gdi.capture_frame()?;
                        self.gdi = Some(gdi);
                        self.frame_id = frame.frame_id;
                        self.last_frame = Instant::now();
                        return Ok(Some(frame));
                    }
                    return Ok(None);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "DXGI next_frame failed; switching to GDI capturer");
                    self.dxgi = None; // Drop DXGI resources
                    let mut gdi = GdiCapturer::new(&self.target)?;
                    let frame = gdi.capture_frame()?;
                    self.gdi = Some(gdi);
                    self.frame_id = frame.frame_id;
                    self.last_frame = Instant::now();
                    return Ok(Some(frame));
                }
            }
        }

        // If neither was initialized (fallback), try creating GDI
        let mut gdi = GdiCapturer::new(&self.target)?;
        let frame = gdi.capture_frame()?;
        self.gdi = Some(gdi);
        self.frame_id = frame.frame_id;
        self.last_frame = Instant::now();
        Ok(Some(frame))
    }

    fn next_frame_dxgi(
        &mut self,
        dxgi: &mut DxgiState,
        timeout: Duration,
    ) -> Result<Option<CapturedFrame>> {
        let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        let acquired = unsafe {
            dxgi.duplication
                .AcquireNextFrame(timeout_ms, &mut info, &mut resource)
        };
        if let Err(error) = acquired {
            if error.code() == DXGI_ERROR_WAIT_TIMEOUT {
                return Ok(None);
            }
            return Err(dxgi_failure("AcquireNextFrame", error));
        }

        Self::update_pointer(dxgi, &info)?;

        let source_desc = match resource.take() {
            Some(res) => {
                let texture: ID3D11Texture2D = res.cast()?;
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                unsafe { texture.GetDesc(&mut desc) };
                if desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
                    drop(texture);
                    drop(res);
                    let _ = unsafe { dxgi.duplication.ReleaseFrame() };
                    return Err(anyhow!("unsupported desktop format: {:?}", desc.Format));
                }
                Self::ensure_staging(dxgi, &desc)?;
                let staging = dxgi.staging.as_ref().context("staging texture missing")?;
                unsafe { dxgi.context.CopyResource(staging, &texture) };
                drop(texture);
                drop(res);
                desc
            }
            None => {
                let _ = unsafe { dxgi.duplication.ReleaseFrame() };
                return Err(anyhow!("desktop duplication returned no resource"));
            }
        };

        // Release the acquired frame back to DWM immediately!
        let released = unsafe { dxgi.duplication.ReleaseFrame() }
            .map_err(|error| dxgi_failure("ReleaseFrame", error));
        released?;

        // Read staging texture into CPU buffer (outside the duplication lock)
        let staging = dxgi.staging.as_ref().context("staging texture missing")?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            dxgi.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|error| dxgi_failure("Map staging texture", error))?
        };
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
        unsafe { dxgi.context.Unmap(staging, 0) };

        let mut frame = CapturedFrame {
            width: source_desc.Width,
            height: source_desc.Height,
            stride: row_len as u32,
            pixels,
            frame_id: self.frame_id + 1,
        };

        if dxgi.pointer_visible {
            if let Some(shape) = &dxgi.pointer_shape {
                shape.composite(&mut frame, dxgi.pointer_position);
            }
        }

        Ok(Some(frame))
    }

    fn update_pointer(dxgi: &mut DxgiState, info: &DXGI_OUTDUPL_FRAME_INFO) -> Result<()> {
        if info.LastMouseUpdateTime != 0 {
            let visible = info.PointerPosition.Visible.as_bool();
            dxgi.pointer_position = info.PointerPosition.Position;
            dxgi.pointer_visible = visible;
        }
        if info.PointerShapeBufferSize != 0 {
            let mut bytes = vec![0; info.PointerShapeBufferSize as usize];
            let mut required = 0;
            let mut shape_info = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
            unsafe {
                dxgi.duplication.GetFramePointerShape(
                    bytes.len() as u32,
                    bytes.as_mut_ptr().cast(),
                    &mut required,
                    &mut shape_info,
                )
            }
            .map_err(|error| dxgi_failure("GetFramePointerShape", error))?;
            dxgi.pointer_shape = Some(PointerShape::new(shape_info, bytes)?);
        }
        Ok(())
    }

    fn ensure_staging(dxgi: &mut DxgiState, source_desc: &D3D11_TEXTURE2D_DESC) -> Result<()> {
        let recreate = dxgi.staging.as_ref().is_none_or(|staging| {
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
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
            };
            let mut staging = None;
            unsafe {
                dxgi.device
                    .CreateTexture2D(&staging_desc, None, Some(&mut staging))?
            };
            dxgi.staging = Some(staging.context("failed to create staging texture")?);
        }
        Ok(())
    }
}

fn wide_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}
