//! Native borderless resizing, constrained before Windows applies the new size.
use anyhow::{bail, Result};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::window::Window;

const SUBCLASS_ID: usize = 0x56445753;

struct ResizeState { dimensions: AtomicU64, fullscreen: AtomicBool }
pub struct WindowSizing { state: Arc<ResizeState> }

impl WindowSizing {
    pub fn new() -> Self {
        Self { state: Arc::new(ResizeState { dimensions: AtomicU64::new(pack(1920, 1080)), fullscreen: AtomicBool::new(false) }) }
    }

    pub fn set_fullscreen(&self, fullscreen: bool) { self.state.fullscreen.store(fullscreen, Ordering::Relaxed); }

    pub fn restore_size(&self, window: &Window, bounds: PhysicalSize<u32>) {
        let dimensions = self.state.dimensions.load(Ordering::Relaxed);
        let ratio = f64::from((dimensions >> 32) as u32) / f64::from(dimensions as u32);
        let _ = window.request_inner_size(fit_size(bounds, ratio, minimum_height(ratio, window.scale_factor())));
    }

    pub fn install(&self, window: &Window) -> Result<()> {
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            bail!("native aspect resize requires a Windows window");
        };
        // The subclass owns one Arc until WM_NCDESTROY. Atomic state avoids
        // mutable Rust borrows across reentrant Windows messages.
        let state = Arc::into_raw(Arc::clone(&self.state));
        if !unsafe { SetWindowSubclass(HWND(handle.hwnd.get() as _), Some(window_proc), SUBCLASS_ID, state as usize) }.as_bool() {
            unsafe { drop(Arc::from_raw(state)); }
            bail!("failed to install aspect ratio resize handler");
        }
        Ok(())
    }

    pub fn set_content_size(&self, window: &Window, width: u32, height: u32, initial: bool) {
        let width = width.max(1);
        let height = height.max(1);
        let old = self.state.dimensions.swap(pack(width, height), Ordering::Relaxed);
        if !initial && u64::from((old >> 32) as u32) * u64::from(height) == u64::from(width) * u64::from(old as u32) { return; }
        let ratio = f64::from(width) / f64::from(height);
        let min_height = minimum_height(ratio, window.scale_factor());
        window.set_min_inner_size(Some(PhysicalSize::new((min_height * ratio).round() as u32, min_height.round() as u32)));
        if window.is_maximized() || self.state.fullscreen.load(Ordering::Relaxed) { return; }
        let monitor = window.current_monitor();
        let bounds = if initial {
            monitor.as_ref().map(|m| {
                let size = m.size();
                PhysicalSize::new((f64::from(size.width) * 0.8) as u32, (f64::from(size.height) * 0.8) as u32)
            }).unwrap_or(PhysicalSize::new(960, 540))
        } else { window.inner_size() };
        let size = fit_size(bounds, ratio, min_height);
        let _ = window.request_inner_size(size);
        if initial {
            if let Some(monitor) = monitor {
                let origin = monitor.position();
                let screen = monitor.size();
                window.set_outer_position(PhysicalPosition::new(
                    origin.x + ((i64::from(screen.width) - i64::from(size.width)) / 2) as i32,
                    origin.y + ((i64::from(screen.height) - i64::from(size.height)) / 2) as i32));
            }
        }
        tracing::info!(width, height, window_width = size.width, window_height = size.height,
            "window aspect ratio updated from capture display");
    }
}

fn pack(width: u32, height: u32) -> u64 { (u64::from(width) << 32) | u64::from(height) }
fn minimum_height(ratio: f64, scale: f64) -> f64 { 180.0 * scale / ratio.min(1.0) }

fn fit_size(bounds: PhysicalSize<u32>, ratio: f64, min_height: f64) -> PhysicalSize<u32> {
    let height = f64::from(bounds.height).min(f64::from(bounds.width) / ratio).max(min_height);
    PhysicalSize::new((height * ratio).round() as u32, height.round() as u32)
}

fn edge_at(rect: RECT, x: i32, y: i32, border: i32) -> Option<u32> {
    if x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom { return None; }
    let left = x < rect.left + border;
    let right = x >= rect.right - border;
    let top = y < rect.top + border;
    let bottom = y >= rect.bottom - border;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(HTTOPLEFT),
        (_, true, true, _) => Some(HTTOPRIGHT),
        (true, _, _, true) => Some(HTBOTTOMLEFT),
        (_, true, _, true) => Some(HTBOTTOMRIGHT),
        (true, _, _, _) => Some(HTLEFT),
        (_, true, _, _) => Some(HTRIGHT),
        (_, _, true, _) => Some(HTTOP),
        (_, _, _, true) => Some(HTBOTTOM),
        _ => None,
    }
}

fn constrain(rect: &mut RECT, edge: u32, ratio: f64, min_height: f64, chrome: (i32, i32)) {
    let width = f64::from((rect.right - rect.left - chrome.0).max(1));
    let height = f64::from((rect.bottom - rect.top - chrome.1).max(1));
    let height = match edge {
        WMSZ_LEFT | WMSZ_RIGHT => width / ratio,
        WMSZ_TOP | WMSZ_BOTTOM => height,
        // Closest point on w=ratio*h allows both drag axes to drive a corner.
        _ => (ratio * width + height) / (ratio * ratio + 1.0),
    }.max(min_height);
    let new_width = (height * ratio).round() as i32 + chrome.0;
    let new_height = height.round() as i32 + chrome.1;
    if matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT) {
        rect.left = rect.right - new_width;
    } else { rect.right = rect.left + new_width; }
    if matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT) {
        rect.top = rect.bottom - new_height;
    } else { rect.bottom = rect.top + new_height; }
}

unsafe extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, id: usize, data: usize) -> LRESULT {
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(window_proc), id);
        drop(Arc::from_raw(data as *const ResizeState));
        return DefSubclassProc(hwnd, message, wparam, lparam);
    }
    if (&*(data as *const ResizeState)).fullscreen.load(Ordering::Relaxed) {
        if message == WM_NCHITTEST { return LRESULT(HTCLIENT as isize); }
        return DefSubclassProc(hwnd, message, wparam, lparam);
    }
    if message == WM_NCHITTEST && !IsZoomed(hwnd).as_bool() {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_ok() {
            // Sign extension is essential on monitors with negative coordinates.
            let x = lparam.0 as i16 as i32;
            let y = (lparam.0 >> 16) as i16 as i32;
            let border = (8.0 * f64::from(GetDpiForWindow(hwnd).max(96)) / 96.0).round() as i32;
            if let Some(edge) = edge_at(rect, x, y, border) { return LRESULT(edge as isize); }
        }
    }
    if message == WM_SIZING && lparam.0 != 0 && (WMSZ_LEFT..=WMSZ_BOTTOMRIGHT).contains(&(wparam.0 as u32)) {
        let dimensions = (&*(data as *const ResizeState)).dimensions.load(Ordering::Relaxed);
        let ratio = f64::from((dimensions >> 32) as u32) / f64::from(dimensions as u32);
        let mut outer = RECT::default();
        let mut client = RECT::default();
        let chrome = if GetWindowRect(hwnd, &mut outer).is_ok() && GetClientRect(hwnd, &mut client).is_ok() {
            ((outer.right - outer.left - client.right).max(0), (outer.bottom - outer.top - client.bottom).max(0))
        } else { (0, 0) };
        let scale = f64::from(GetDpiForWindow(hwnd).max(96)) / 96.0;
        constrain(&mut *(lparam.0 as *mut RECT), wparam.0 as u32, ratio, minimum_height(ratio, scale), chrome);
        return LRESULT(1);
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_edges_keep_ratio_and_opposite_anchor() {
        for ratio in [16.0 / 9.0, 9.0 / 16.0, 1.0, 21.0 / 9.0] {
            for edge in WMSZ_LEFT..=WMSZ_BOTTOMRIGHT {
                let before = RECT { left: -700, top: -300, right: 301, bottom: 477 };
                let mut rect = before;
                constrain(&mut rect, edge, ratio, minimum_height(ratio, 1.0), (0, 0));
                assert!((f64::from(rect.right - rect.left) - ratio * f64::from(rect.bottom - rect.top)).abs() <= (1.0 + ratio) / 2.0);
                if matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT) { assert_eq!(rect.right, before.right); }
                else { assert_eq!(rect.left, before.left); }
                if matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT) { assert_eq!(rect.bottom, before.bottom); }
                else { assert_eq!(rect.top, before.top); }
            }
        }
    }
    #[test]
    fn hit_tests_edges_and_corners_at_negative_origins() {
        let rect = RECT { left: -1000, top: -500, right: -200, bottom: -50 };
        for (x, y, expected) in [(-999,-499,HTTOPLEFT),(-201,-499,HTTOPRIGHT),(-999,-51,HTBOTTOMLEFT),
            (-201,-51,HTBOTTOMRIGHT),(-999,-300,HTLEFT),(-201,-300,HTRIGHT),(-600,-499,HTTOP),(-600,-51,HTBOTTOM)] {
            assert_eq!(edge_at(rect,x,y,8),Some(expected));
        }
        assert_eq!(edge_at(rect,-600,-300,8),None);
        assert_eq!(edge_at(rect,-1001,-300,8),None);
    }
    #[test]
    fn minimum_and_nonclient_border_preserve_client_ratio() {
        let mut rect = RECT { left: 0, top: 0, right: 20, bottom: 20 };
        constrain(&mut rect, WMSZ_BOTTOMRIGHT, 16.0/9.0, 270.0, (16, 8));
        assert_eq!((rect.right, rect.bottom), (496, 278));
    }
    #[test]
    fn initial_landscape_and_portrait_sizes_fit_available_area() {
        assert_eq!(fit_size(PhysicalSize::new(1000,800),16.0/9.0,180.0),PhysicalSize::new(1000,563));
        assert_eq!(fit_size(PhysicalSize::new(1000,800),9.0/16.0,320.0),PhysicalSize::new(450,800));
    }
}
