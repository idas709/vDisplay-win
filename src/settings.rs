//! Modeless settings window. Native controls live here; applying settings stays in App.
use anyhow::Result;
use crate::app_icon;
use crate::language::Language;
use parsec_vdd_rust::display::Mode;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::{atomic::{AtomicIsize, Ordering}, Arc, Mutex};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use winit::{dpi::LogicalSize, event_loop::ActiveEventLoop, window::{Window, WindowAttributes, WindowButtons}};
use winit::platform::windows::WindowAttributesExtWindows;

pub type ResolutionOptions = Result<(Vec<Mode>, Option<Mode>)>;
pub enum Action {
    Accept {
        language: Language,
        resolution: Option<Mode>,
        view: Option<crate::viewport::ResetMode>,
        window_size: Option<(u32, u32)>,
    },
    Refresh,
    Close,
}
const APPLY: usize = 1;
const CLOSE: usize = 2;
const FIT: usize = 3;
const ACTUAL: usize = 4;
const REFRESH: usize = 5;
const WIDTH_EDIT: usize = 7;
const HEIGHT_EDIT: usize = 8;
const RESOLUTION: usize = 10;
const LANGUAGE_COMBO: usize = 11;
const SUBCLASS: usize = 0x56445345;

struct Commands {
    queue: Mutex<Vec<usize>>,
    wake: winit::event_loop::EventLoopProxy<()>,
    updating_size: std::sync::atomic::AtomicBool,
    content_ratio: std::sync::atomic::AtomicU64,
    width_edit: AtomicIsize,
    height_edit: AtomicIsize,
}

pub struct SettingsWindow {
    pub window: Window,
    pub target_window_id: winit::window::WindowId,
    language: Language,
    language_combo: HWND,
    hwnd: HWND,
    combo: HWND,
    current: Option<Mode>,
    view_buttons: [HWND; 3],
    status: HWND,
    width_edit: HWND,
    height_edit: HWND,
    controls: Vec<(HWND, [i32; 4])>,
    font: HFONT,
    latin_font: HFONT,
    commands: Arc<Commands>,
    modes: Vec<Mode>,
    hook_hwnd: Arc<AtomicIsize>,
}

impl SettingsWindow {
    pub fn new(
        event_loop: &ActiveEventLoop,
        owner: &Window,
        target_window_id: winit::window::WindowId,
        initial_window_size: winit::dpi::PhysicalSize<u32>,
        content_ratio: f64,
        hook_hwnd: Arc<AtomicIsize>,
        wake: winit::event_loop::EventLoopProxy<()>,
        language: Language,
    ) -> Result<Self> {
        let RawWindowHandle::Win32(owner_handle) = owner.window_handle()?.as_raw() else { anyhow::bail!("Windows window required") };
        let window = event_loop.create_window(WindowAttributes::default()
            .with_title(language.text("設定 - Virtual Display Workspace", "Settings - Virtual Display Workspace"))
            .with_window_icon(Some(app_icon::load()?))
            .with_inner_size(LogicalSize::new(540., 630.))
            .with_resizable(false).with_maximized(false).with_visible(false)
            .with_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE)
            .with_owner_window(owner_handle.hwnd.get() as _))?;
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else { anyhow::bail!("Windows window required") };
        let hwnd = HWND(handle.hwnd.get() as _);
        let commands = Arc::new(Commands {
            queue: Mutex::new(Vec::new()),
            wake,
            updating_size: std::sync::atomic::AtomicBool::new(false),
            content_ratio: std::sync::atomic::AtomicU64::new(content_ratio.to_bits()),
            width_edit: AtomicIsize::new(0),
            height_edit: AtomicIsize::new(0),
        });
        let state = Arc::into_raw(Arc::clone(&commands));
        if !unsafe { SetWindowSubclass(hwnd, Some(settings_proc), SUBCLASS, state as usize) }.as_bool() {
            unsafe { drop(Arc::from_raw(state)); }
            anyhow::bail!("設定ウィンドウを初期化できませんでした。");
        }
        let mut result = Self {
            window,
            target_window_id,
            language,
            language_combo: HWND::default(),
            hwnd,
            combo: HWND::default(),
            current: None,
            view_buttons: [HWND::default(); 3],
            status: HWND::default(),
            width_edit: HWND::default(),
            height_edit: HWND::default(),
            controls: Vec::new(),
            font: HFONT::default(),
            latin_font: HFONT::default(),
            commands,
            modes: Vec::new(),
            hook_hwnd,
        };
        // Sections and controls are separate from the application-side settings actions.
        result.add("STATIC", language.text("ディスプレイ", "Display"), 0, 0, [24, 20, 480, 24])?;
        result.add("STATIC", language.text("仮想ディスプレイの解像度", "Virtual display resolution"), 0, 0, [24, 54, 320, 22])?;
        result.combo = result.add("COMBOBOX", "", RESOLUTION, CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0 | WS_VSCROLL.0, [24, 80, 300, 240])?;

        result.add("BUTTON", language.text("再取得(&R)", "Refresh"), REFRESH, WS_TABSTOP.0, [430, 78, 86, 30])?;
        result.status = result.add("STATIC", "", 0, 0, [24, 114, 492, 50])?;

        // Window size controls
        result.add("STATIC", language.text("ウィンドウサイズ", "Window size"), 0, 0, [24, 168, 492, 24])?;
        result.add("STATIC", language.text("幅", "Width:"), 0, 0, [24, 198, 45, 22])?;
        result.width_edit = result.add("EDIT", "", WIDTH_EDIT, ES_NUMBER as u32 | ES_AUTOHSCROLL as u32 | WS_BORDER.0 | WS_TABSTOP.0, [72, 195, 95, 26])?;
        result.add("STATIC", "px  ×", 0, 0, [173, 198, 45, 22])?;
        result.add("STATIC", language.text("高さ", "Height:"), 0, 0, [226, 198, 48, 22])?;
        result.height_edit = result.add("EDIT", "", HEIGHT_EDIT, ES_NUMBER as u32 | ES_AUTOHSCROLL as u32 | WS_BORDER.0 | WS_TABSTOP.0, [278, 195, 95, 26])?;
        result.add("STATIC", "px", 0, 0, [379, 198, 30, 22])?;
        result.add("STATIC", language.text("※一方の数値を変更すると、アスペクト比に合わせて他方が自動計算されます。", "* Changing either value automatically calculates the other based on aspect ratio."), 0, 0, [24, 226, 492, 20])?;

        // Populate initial window sizes
        let w_str = wide(&initial_window_size.width.to_string());
        let h_str = wide(&initial_window_size.height.to_string());
        unsafe {
            let _ = SetWindowTextW(result.width_edit, PCWSTR(w_str.as_ptr()));
            let _ = SetWindowTextW(result.height_edit, PCWSTR(h_str.as_ptr()));
        }
        result.commands.width_edit.store(result.width_edit.0 as isize, Ordering::Relaxed);
        result.commands.height_edit.store(result.height_edit.0 as isize, Ordering::Relaxed);

        result.add("STATIC", language.text("表示", "View"), 0, 0, [24, 254, 492, 24])?;
        for (index, (label, id)) in [(language.text("現在の表示を維持", "Keep current view"), 6), (language.text("ウィンドウに合わせる", "Fit to window"), FIT), (language.text("実寸表示", "Actual size"), ACTUAL)].into_iter().enumerate() {
            result.view_buttons[index] = result.add("BUTTON", label, id, BS_AUTORADIOBUTTON as u32 | WS_TABSTOP.0 | if index == 0 { WS_GROUP.0 } else { 0 }, [24, 280 + index as i32 * 26, 360, 24])?;
        }
        unsafe { send(result.view_buttons[0], BM_SETCHECK, WPARAM(1), LPARAM(0)); }

        result.add("STATIC", language.text("言語 / Language", "Language / 言語"), 0, 0, [24, 366, 300, 24])?;
        result.language_combo = result.add("COMBOBOX", "", LANGUAGE_COMBO, CBS_DROPDOWNLIST as u32 | WS_TABSTOP.0 | WS_GROUP.0, [24, 394, 300, 120])?;
        for label in ["日本語", "English"] {
            let label = wide(label);
            unsafe { send(result.language_combo, CB_ADDSTRING, WPARAM(0), LPARAM(label.as_ptr() as isize)); }
        }
        unsafe { send(result.language_combo, CB_SETCURSEL, WPARAM(if language == Language::English { 1 } else { 0 }), LPARAM(0)); }

        result.add("STATIC", language.text("このアプリについて", "About this app"), 0, 0, [24, 442, 492, 24])?;
        let about = format!(
            "Virtual Display Workspace  {}\nAuthor: {}    License: {}",
            env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_AUTHORS"), env!("CARGO_PKG_LICENSE"),
        );
        result.add("STATIC", &about, 0, 0, [24, 470, 492, 52])?;

        result.add("BUTTON", "OK", APPLY, BS_DEFPUSHBUTTON as u32 | WS_TABSTOP.0 | WS_GROUP.0, [300, 546, 104, 32])?;
        result.add("BUTTON", language.text("キャンセル", "Cancel"), CLOSE, WS_TABSTOP.0, [412, 546, 104, 32])?;
        result.layout();
        result.hook_hwnd.store(hwnd.0 as isize, Ordering::Relaxed);
        Ok(result)
    }

    fn add(&mut self, class: &str, text: &str, id: usize, style: u32, rect: [i32; 4]) -> Result<HWND> {
        let class = wide(class);
        let text = wide(text);
        let child = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), PCWSTR(class.as_ptr()), PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style), 0, 0, 1, 1, Some(self.hwnd), Some(HMENU(id as _)), None, None) }?;
        self.controls.push((child, rect));
        Ok(child)
    }

    pub fn layout(&mut self) {
        let scale = self.window.scale_factor();
        let font = unsafe { CreateFontW(-(14. * scale).round() as i32, 0, 0, 0, 400, 0, 0, 0,
            DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Yu Gothic UI")) };
        let latin_font = unsafe { CreateFontW(-(14. * scale).round() as i32, 0, 0, 0, 400, 0, 0, 0,
            DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI")) };
        for (child, rect) in &self.controls {
            unsafe {
                let _ = MoveWindow(*child, (rect[0] as f64*scale).round() as i32, (rect[1] as f64*scale).round() as i32,
                    (rect[2] as f64*scale).round() as i32, (rect[3] as f64*scale).round() as i32, true);
                let mut label = [0u16; 512];
                let count = GetWindowTextW(*child, &mut label);
                let latin = *child == self.combo || *child == self.width_edit || *child == self.height_edit || (count > 0 && label[..count as usize].iter().all(|c| *c < 128));
                send(*child, WM_SETFONT, WPARAM(if latin { latin_font.0 } else { font.0 } as usize), LPARAM(1));
            }
        }
        if !self.font.is_invalid() { unsafe { let _ = DeleteObject(self.font.into()); } }
        if !self.latin_font.is_invalid() { unsafe { let _ = DeleteObject(self.latin_font.into()); } }
        self.font = font; self.latin_font = latin_font;
    }

    pub fn refresh(&mut self, options: ResolutionOptions) {
        unsafe { send(self.combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0)); }
        self.modes.clear();
        self.current = None;
        let message = match options {
            Ok((modes, current)) if !modes.is_empty() => {
                self.current = current.clone();
                self.modes = modes;
                let selected = self.modes.iter().position(|mode| current.as_ref().is_some_and(|c| c.width == mode.width && c.height == mode.height));
                for mode in &self.modes {
                    let label = wide(&format!("{} × {}", mode.width, mode.height));
                    unsafe { send(self.combo, CB_ADDSTRING, WPARAM(0), LPARAM(label.as_ptr() as isize)); }
                }
                unsafe { send(self.combo, CB_SETCURSEL, WPARAM(selected.unwrap_or(0)), LPARAM(0)); }
                current.map(|mode| format!("{}: {} × {}\n{}", self.language.text("現在", "Current"), mode.width, mode.height, self.language.text("変更は「OK」を押したときに適用されます。", "Changes are applied when you press OK."))).unwrap_or_default()
            }
            Ok(_) => self.language.text("対応する仮想解像度がありません。「再取得」で接続状態を確認できます。", "No supported resolutions. Use Refresh to check the connection.").to_string(),
            Err(error) => error.to_string(),
        };
        unsafe {
            let _ = EnableWindow(self.combo, !self.modes.is_empty());

        }
        self.set_status(&message);
    }

    pub fn set_status(&self, message: &str) {
        let text = wide(&self.language.error(message));
        unsafe {
            let _ = SetWindowTextW(self.status, PCWSTR(text.as_ptr()));
            let font = if self.language == Language::English { self.latin_font } else { self.font };
            send(self.status, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
        }
    }

    pub fn show(&self) {
        self.window.set_minimized(false);
        self.window.set_visible(true);
        self.window.focus_window();
        unsafe { let _ = SetFocus(Some(if self.modes.is_empty() { self.hwnd } else { self.combo })); }
    }

    pub fn actions(&self) -> Vec<Action> {
        let commands = std::mem::take(&mut *self.commands.queue.lock().unwrap_or_else(|e| e.into_inner()));
        commands.into_iter().filter_map(|command| match command {
            APPLY => {
                let selected = unsafe { send(self.combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)) }.0;
                let mut w_buf = [0u16; 32];
                let w_len = unsafe { GetWindowTextW(self.width_edit, &mut w_buf) };
                let mut h_buf = [0u16; 32];
                let h_len = unsafe { GetWindowTextW(self.height_edit, &mut h_buf) };
                let window_size = if w_len > 0 && h_len > 0 {
                    let w_str = String::from_utf16_lossy(&w_buf[..w_len as usize]);
                    let h_str = String::from_utf16_lossy(&h_buf[..h_len as usize]);
                    match (w_str.trim().parse::<u32>(), h_str.trim().parse::<u32>()) {
                        (Ok(w), Ok(h)) if w > 0 && h > 0 => Some((w, h)),
                        _ => None,
                    }
                } else {
                    None
                };
                Some(Action::Accept {
                    language: if unsafe { send(self.language_combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)) }.0 == 1 { Language::English } else { Language::Japanese },
                    resolution: self.modes.get(selected as usize).filter(|mode| Some(*mode) != self.current.as_ref()).cloned(),
                    view: if unsafe { send(self.view_buttons[1], BM_GETCHECK, WPARAM(0), LPARAM(0)) }.0 == 1 {
                        Some(crate::viewport::ResetMode::Fit)
                    } else if unsafe { send(self.view_buttons[2], BM_GETCHECK, WPARAM(0), LPARAM(0)) }.0 == 1 {
                        Some(crate::viewport::ResetMode::ActualSize)
                    } else { None },
                    window_size,
                })
            }
            CLOSE => Some(Action::Close), REFRESH => Some(Action::Refresh), _ => None,
        }).collect()
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        self.hook_hwnd.store(0, Ordering::Relaxed);
        // Destroy controls before releasing their font; Window releases its HWND afterward.
        for (child, _) in self.controls.drain(..) { unsafe { let _ = DestroyWindow(child); } }
        if !self.latin_font.is_invalid() { unsafe { let _ = DeleteObject(self.latin_font.into()); } }
        if !self.font.is_invalid() { unsafe { let _ = DeleteObject(self.font.into()); } }
    }
}

fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() }

unsafe extern "system" fn settings_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, id: usize, data: usize) -> LRESULT {
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(settings_proc), id);
        drop(Arc::from_raw(data as *const Commands));
        return DefSubclassProc(hwnd, message, wparam, lparam);
    }
    if message == WM_COMMAND {
        let code = (wparam.0 >> 16) as u16;
        let id = (wparam.0 & 0xffff) as usize;
        let commands = &*(data as *const Commands);

        // EN_CHANGE = 0x0300: automatically calculate aspect ratio counterpart
        if code == 0x0300 && !commands.updating_size.load(Ordering::Relaxed) {
            let width_hwnd = HWND(commands.width_edit.load(Ordering::Relaxed) as _);
            let height_hwnd = HWND(commands.height_edit.load(Ordering::Relaxed) as _);
            let ratio = f64::from_bits(commands.content_ratio.load(Ordering::Relaxed));
            if !width_hwnd.is_invalid() && !height_hwnd.is_invalid() && ratio > 0.0 {
                if id == WIDTH_EDIT {
                    let mut buf = [0u16; 32];
                    let len = GetWindowTextW(width_hwnd, &mut buf);
                    if len > 0 {
                        let text = String::from_utf16_lossy(&buf[..len as usize]);
                        if let Ok(w) = text.trim().parse::<u32>() {
                            if w > 0 {
                                let h = crate::window_sizing::calculate_aspect_height(w, ratio);
                                commands.updating_size.store(true, Ordering::Relaxed);
                                let h_str = wide(&h.to_string());
                                let _ = SetWindowTextW(height_hwnd, PCWSTR(h_str.as_ptr()));
                                commands.updating_size.store(false, Ordering::Relaxed);
                            }
                        }
                    }
                } else if id == HEIGHT_EDIT {
                    let mut buf = [0u16; 32];
                    let len = GetWindowTextW(height_hwnd, &mut buf);
                    if len > 0 {
                        let text = String::from_utf16_lossy(&buf[..len as usize]);
                        if let Ok(h) = text.trim().parse::<u32>() {
                            if h > 0 {
                                let w = crate::window_sizing::calculate_aspect_width(h, ratio);
                                commands.updating_size.store(true, Ordering::Relaxed);
                                let w_str = wide(&w.to_string());
                                let _ = SetWindowTextW(width_hwnd, PCWSTR(w_str.as_ptr()));
                                commands.updating_size.store(false, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
        }

        if code == BN_CLICKED as u16 {
            commands.queue.lock().unwrap_or_else(|e| e.into_inner()).push(id);
            let _ = commands.wake.send_event(());
            return LRESULT(0);
        }
    }
    if message == WM_ERASEBKGND {
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        FillRect(HDC(wparam.0 as _), &rect, GetSysColorBrush(COLOR_BTNFACE));
        return LRESULT(1);
    }
    if message == WM_PAINT {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        FillRect(dc, &paint.rcPaint, GetSysColorBrush(COLOR_BTNFACE));
        let _ = EndPaint(hwnd, &paint);
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

pub fn message_hook(hwnd: &AtomicIsize, message: *const std::ffi::c_void) -> bool {
    let hwnd = HWND(hwnd.load(Ordering::Relaxed) as _);
    if hwnd.is_invalid() { return false; }
    unsafe { IsDialogMessageW(hwnd, message.cast()).as_bool() }
}

unsafe fn send(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    SendMessageW(hwnd, message, Some(wparam), Some(lparam))
}
