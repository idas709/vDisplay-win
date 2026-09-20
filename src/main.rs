#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capture;
mod app_icon;
mod diagnostics;
mod language;
mod driver;
mod pointer;
mod renderer;
mod settings;
mod ui;
mod zoom_ui;
mod viewport;
mod window_sizing;

use anyhow::Result;
use capture::{DesktopDuplicator, DisplayInfo};
use driver::DriverSession;
use glam::Vec2;
use renderer::Renderer;
use ui::{OverlayAction, OverlayState};
use viewport::{ResetMode, ViewportController};
use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId};

struct App {
    language: language::Language,
    zoom_ui: zoom_ui::ZoomUi,
    settings_proxy: Option<winit::event_loop::EventLoopProxy<()>>,
    settings: Option<settings::SettingsWindow>,
    settings_hwnd: std::sync::Arc<std::sync::atomic::AtomicIsize>,
    window_sizing: window_sizing::WindowSizing,
    window: Option<Window>,
    renderer: Option<Renderer>,
    duplicator: Option<DesktopDuplicator>,
    driver_session: Option<DriverSession>,
    capture_display_name: Option<String>,
    capture_bounds: Option<windows::Win32::Foundation::RECT>,
    next_capture_recovery: std::time::Instant,
    displays: Vec<DisplayInfo>,
    viewport: Option<ViewportController>,
    overlay: OverlayState,
    modifiers: ModifiersState,
    dragging: bool,
    last_cursor: Vec2,
    space_down: bool,
    fullscreen_restore_size: Option<winit::dpi::PhysicalSize<u32>>,
}

impl App {
    fn new() -> Self {
        Self { language: language::Language::load(), zoom_ui: zoom_ui::ZoomUi::new(), settings_proxy: None, settings: None, settings_hwnd: std::sync::Arc::new(std::sync::atomic::AtomicIsize::new(0)), window_sizing: window_sizing::WindowSizing::new(), window: None, renderer: None, duplicator: None, driver_session: None, capture_display_name: None, capture_bounds: None, next_capture_recovery: std::time::Instant::now(), displays: Vec::new(), viewport: None, overlay: OverlayState::new(), modifiers: ModifiersState::empty(), dragging: false, last_cursor: Vec2::ZERO, space_down: false, fullscreen_restore_size: None }
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attributes = WindowAttributes::default()
            .with_title("Virtual Display Workspace")
            .with_window_icon(Some(app_icon::load()?))
            .with_decorations(false)
            .with_resizable(true)
            .with_visible(false);
        let window = event_loop.create_window(attributes)?;
        self.window_sizing.install(&window)?;
        let mut driver_session = None;
        let mut duplicator = None;
        let mut content_size = Vec2::new(1920.0, 1080.0);
        match DriverSession::start() {
            Ok(mut session) => {
                tracing::info!(device_name = %session.display.device_name, "Parsec virtual display added");
                let mut selected_display = None;
                for _ in 0..50 {
                    self.displays = capture::enumerate_displays()?;
                    selected_display = session.refresh_display().and_then(|target| capture::select_parsec_display(&self.displays, &target).cloned());
                    if selected_display.is_some() { break; }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                if let Some(display) = selected_display.as_ref() {
                    content_size = Vec2::new(display.width.max(1) as f32, display.height.max(1) as f32);
                    match DesktopDuplicator::new(display) {
                        Ok(capture) => duplicator = Some(capture),
                        Err(error) => tracing::warn!(error = %format!("{error:#}"), "initial duplication failed; scheduling recovery"),
                    }
                    self.capture_display_name = Some(display.name.clone());
                    self.capture_bounds = Some(display.bounds);
                    driver_session = Some(session);
                } else {
                    tracing::warn!("Parsec VDD started but no DXGI output appeared after 5 seconds");
                    window.set_title("Virtual Display Workspace - waiting for virtual display");
                    driver_session = Some(session);
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Parsec VDD is unavailable; starting setup state");
                window.set_title("Virtual Display Workspace - Parsec VDD required");
            }
        }
        self.window_sizing.set_content_size(&window, content_size.x as u32, content_size.y as u32, true);
        let renderer = pollster::block_on(Renderer::new(&window))?;
        window.set_visible(true);
        let mut viewport = ViewportController::new(content_size);
        let size = window.inner_size();
        viewport.resize(Vec2::new(size.width as f32, size.height as f32));
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.duplicator = duplicator;
        self.driver_session = driver_session;
        self.viewport = Some(viewport);
        Ok(())
    }

    fn recreate_capture(&mut self) {
        if std::time::Instant::now() < self.next_capture_recovery { return; }
        self.next_capture_recovery = std::time::Instant::now() + std::time::Duration::from_millis(500);
        let Some(session) = self.driver_session.as_mut() else { return };
        // Release all old COM references before attempting DuplicateOutput again.
        drop(self.duplicator.take());
        let target = session.refresh_display();
        let display_name = self.capture_display_name.clone().unwrap_or_default();
        let displays = match capture::enumerate_displays() {
            Ok(displays) => displays,
            Err(error) => { tracing::warn!(%error, "failed to enumerate displays while recovering capture"); return; }
        };
        let selected_display = target.as_ref().and_then(|target| capture::select_parsec_display(&displays, target));
        let Some(selected_display) = selected_display else {
            tracing::warn!(%display_name, "owned Parsec output is unavailable, detached or cloned after display topology change");
            self.duplicator = None;
            return;
        };
        if selected_display.name != display_name {
            tracing::info!(old_name = %display_name, new_name = %selected_display.name, "capture output name changed");
            self.capture_display_name = Some(selected_display.name.clone());
            self.capture_bounds = Some(selected_display.bounds);
        }
        match DesktopDuplicator::new(selected_display) {
            Ok(duplicator) => {
                tracing::info!(display_name = %selected_display.name, "desktop duplication recreated");
                self.duplicator = Some(duplicator);
                if let Some(window) = self.window.as_ref() {
                    self.window_sizing.set_content_size(window, selected_display.width, selected_display.height, false);
                }
                if let Some(viewport) = self.viewport.as_mut() {
                    viewport.content_size = Vec2::new(selected_display.width.max(1) as f32, selected_display.height.max(1) as f32);
                    viewport.resize(viewport.viewport_size);
                }
            }
            Err(error) => tracing::warn!(error = %format!("{error:#}"), "failed to recreate desktop duplication"),
        }
    }

    fn toggle_fullscreen(&mut self) {
        let Some(window) = self.window.as_ref() else { return };
        let entering = window.fullscreen().is_none();
        self.window_sizing.set_fullscreen(entering);
        if entering {
            self.fullscreen_restore_size = (!window.is_maximized()).then(|| window.inner_size());
            window.set_fullscreen(Some(Fullscreen::Borderless(window.current_monitor())));
        } else {
            window.set_fullscreen(None);
            if let Some(size) = self.fullscreen_restore_size.take() { self.window_sizing.restore_size(window, size); }
        }
        self.dragging = false;
        tracing::info!(fullscreen = entering, "viewer fullscreen changed");
    }

    fn move_cursor_to_display(&self, cursor: Vec2) {
        let (Some(viewport), Some(bounds)) = (self.viewport.as_ref(), self.capture_bounds) else { return };
        let position = viewport.content_position_at(cursor);
        let x = bounds.left.saturating_add(position.x.round() as i32);
        let y = bounds.top.saturating_add(position.y.round() as i32);
        unsafe { let _ = SetCursorPos(x, y); }
    }

    fn refresh_settings(&mut self) {
        if self.driver_session.is_none() {
            tracing::info!("settings refresh requested while VDD is disconnected; reconnecting");
            match DriverSession::start() {
                Ok(session) => {
                    tracing::info!(device_name = %session.display.device_name, "Parsec VDD reconnected from settings");
                    self.driver_session = Some(session);
                    self.next_capture_recovery = std::time::Instant::now();
                    self.recreate_capture();
                }
                Err(error) => tracing::warn!(error = %format!("{error:#}"), "Parsec VDD reconnect failed"),
            }
        }
        let options = self.driver_session.as_mut().ok_or_else(|| anyhow::anyhow!("Parsec仮想ディスプレイが接続されていません。"))
            .and_then(|session| session.resolution_options());
        if let Some(settings) = self.settings.as_mut() { settings.refresh(options); }
    }

    fn process_settings(&mut self) {
        let actions = self.settings.as_ref().map(|settings| settings.actions()).unwrap_or_default();
        for action in actions {
            if self.settings.is_none() { break; }
            match action {
                settings::Action::Close => { self.settings = None; }
                settings::Action::Refresh => self.refresh_settings(),
                settings::Action::Accept { language, resolution, view } => {
                    if let Some(mode) = resolution {
                        drop(self.duplicator.take());
                        let result = self.driver_session.as_mut().ok_or_else(|| anyhow::anyhow!("仮想ディスプレイが切断されました。"))
                            .and_then(|session| session.set_resolution(&mode));
                        self.next_capture_recovery = std::time::Instant::now();
                        self.recreate_capture();
                        if let Err(error) = result {
                            tracing::warn!(%error, "virtual resolution change failed");
                            if let Some(settings) = &self.settings { settings.set_status(&error.to_string()); }
                            continue;
                        }
                    }
                    if let (Some(viewport), Some(mode)) = (self.viewport.as_mut(), view) { viewport.reset(mode); }
                    if language != self.language {
                        if let Err(error) = language.save() {
                            if let Some(settings) = &self.settings { settings.set_status(&format!("{}: {error}", self.language.text("言語を保存できませんでした", "Could not save the language"))); }
                            continue;
                        }
                        self.language = language;
                    }
                    self.settings = None;
                }
            }
        }
    }

    fn handle_overlay(&mut self, action: OverlayAction, event_loop: &ActiveEventLoop) {
        tracing::debug!(?action, "caption action invoked");
        let Some(window) = self.window.as_ref() else { return };
        match action {
            OverlayAction::MoveWindow => {}
            OverlayAction::Settings => {
                if self.settings.is_none() {
                    match settings::SettingsWindow::new(event_loop, window, std::sync::Arc::clone(&self.settings_hwnd), self.settings_proxy.as_ref().unwrap().clone(), self.language) {
                        Ok(settings) => { self.settings = Some(settings); self.refresh_settings(); },
                        Err(error) => { ui::show_error(window, &error.to_string(), self.language); return; }
                    }
                }
                if let Some(settings) = &self.settings { settings.show(); }
            },
            OverlayAction::Fullscreen => self.toggle_fullscreen(),
            OverlayAction::Minimize => window.set_minimized(true),
            OverlayAction::MaximizeRestore => {
                if window.fullscreen().is_some() { self.toggle_fullscreen(); }
                else { window.set_maximized(!window.is_maximized()); }
            },
            OverlayAction::Close => event_loop.exit(),
        }
    }

    fn redraw(&mut self) {
        let Some(window_size) = self.window.as_ref().map(|window| window.inner_size()) else { return };
        let mut capture_lost = self.duplicator.is_none();
        if let Some(duplicator) = self.duplicator.as_mut() {
            match duplicator.next_frame(std::time::Duration::from_millis(1)) {
                Ok(Some(frame)) => {
                    if frame.frame_id == 1 { tracing::debug!(width = frame.width, height = frame.height, "uploading first frame to renderer"); }
                    if let (Some(renderer), Some(viewport)) = (self.renderer.as_mut(), self.viewport.as_ref()) {
                        renderer.update_frame(&frame);
                        renderer.update_viewport(viewport);
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, "desktop duplication lost; scheduling recovery");
                    capture_lost = true;
                }
            }
        }
        if capture_lost { drop(self.duplicator.take()); self.recreate_capture(); }
        if let (Some(window), Some(viewport)) = (self.window.as_ref(), self.viewport.as_mut()) { self.zoom_ui.update(window, viewport); }
        self.zoom_ui.language = self.language;
        let overlay_visual = self.window.as_ref().map(|window| self.overlay.update(window));
        if let Some(renderer) = self.renderer.as_mut() {
            if let Some(viewport) = self.viewport.as_ref() { renderer.update_viewport(viewport); }
            match renderer.render(overlay_visual.as_ref().expect("viewer window exists"), &self.zoom_ui, self.viewport.as_ref().unwrap()) {
                Ok(()) => {}
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => { renderer.resize(window_size.width, window_size.height); }
                Err(wgpu::SurfaceError::OutOfMemory) => std::process::exit(1),
                Err(wgpu::SurfaceError::Timeout) => {}
            }
        }
        if let Some(window) = self.window.as_ref() { window.request_redraw(); }
    }
}

impl ApplicationHandler for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, (): ()) { self.process_settings(); }
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) { self.process_settings(); }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            if let Err(error) = self.initialize(event_loop) { tracing::error!(%error, "failed to initialize viewer"); event_loop.exit(); }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        if self.settings.as_ref().is_some_and(|settings| settings.window.id() == window_id) {
            match event {
                WindowEvent::CloseRequested => self.settings = None,
                WindowEvent::ScaleFactorChanged { .. } => { if let Some(settings) = self.settings.as_mut() { settings.layout(); } }
                _ => {}
            }
            return;
        }
        if !self.window.as_ref().is_some_and(|window| window.id() == window_id) { return; }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() { renderer.resize(size.width, size.height); }
                if let Some(viewport) = self.viewport.as_mut() { viewport.resize(Vec2::new(size.width as f32, size.height as f32)); }
            }
            WindowEvent::Focused(false) => { self.overlay.cancel_press(); self.zoom_ui.cancel(); self.dragging = false; },
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event: KeyEvent { physical_key: PhysicalKey::Code(code), state, repeat, .. }, .. } => {
                let pressed = state == ElementState::Pressed;
                if pressed && code == KeyCode::Escape && self.zoom_ui.escape() { return; }
                if pressed && !repeat && (code == KeyCode::F11 || (code == KeyCode::Escape && self.window.as_ref().is_some_and(|window| window.fullscreen().is_some()))) { self.toggle_fullscreen(); }
                if code == KeyCode::Space { self.space_down = pressed; }
                if pressed && self.modifiers.control_key() && code == KeyCode::Digit0 { if let Some(viewport) = self.viewport.as_mut() { viewport.reset(if self.modifiers.shift_key() { ResetMode::ActualSize } else { ResetMode::Fit }); } }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let (Some(window), Some(viewport)) = (self.window.as_ref(), self.viewport.as_mut()) {
                    self.zoom_ui.update(window, viewport);
                    if self.zoom_ui.mouse(state, button, viewport) { self.dragging = false; return; }
                }
                if let Some(window) = self.window.as_ref() { self.overlay.update(window); }
                let (consumed, action) = self.overlay.mouse_input(state, button);
                if consumed {
                    self.dragging = false;
                            if let Some(action) = action {
                                if action == OverlayAction::MoveWindow {
                                    if let Some(window) = self.window.as_ref() { let _ = window.drag_window(); }
                                } else {
                                    self.handle_overlay(action, event_loop);
                                }
                            }
                    return;
                }
                if state == ElementState::Pressed && button == MouseButton::Left && self.modifiers.control_key() {
                    self.move_cursor_to_display(self.last_cursor);
                    self.dragging = false;
                    return;
                }
                let zoomed = self.viewport.as_ref().is_some_and(|viewport| viewport.zoom > 1.0);
                self.dragging = state == ElementState::Pressed
                    && (button == MouseButton::Middle || (button == MouseButton::Left && (self.space_down || zoomed)));
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor = Vec2::new(position.x as f32, position.y as f32);
                if let Some(viewport) = self.viewport.as_mut() {
                    if self.zoom_ui.cursor_moved(cursor, viewport) {
                        self.last_cursor = cursor;
                        return;
                    }
                }
                if self.dragging {
                    if let Some(viewport) = self.viewport.as_mut() { viewport.pan_by(cursor - self.last_cursor); }
                }
                self.last_cursor = cursor;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let (Some(window), Some(viewport)) = (self.window.as_ref(), self.viewport.as_mut()) {
                    self.zoom_ui.update(window, viewport);
                    let amount = match delta { MouseScrollDelta::LineDelta(_, y) => y, MouseScrollDelta::PixelDelta(p) => p.y as f32 };
                    if self.zoom_ui.wheel(amount, viewport) { return; }
                }
                let movement = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y) * 48.0,
                    MouseScrollDelta::PixelDelta(position) => Vec2::new(position.x as f32, position.y as f32),
                };
                if let Some(viewport) = self.viewport.as_mut() {
                    if self.modifiers.control_key() {
                        let amount = match delta {
                            MouseScrollDelta::LineDelta(_, y) => y,
                            MouseScrollDelta::PixelDelta(position) => position.y as f32 / 100.0,
                        };
                        viewport.zoom_at(amount, self.last_cursor);
                    } else {
                        viewport.pan_by(movement);
                    }
                }
            }
            _ => {}
        }
    }
}

// Setup/uninstall waits until every viewer releases this named kernel object.
struct InstallerMutex(windows::Win32::Foundation::HANDLE);
impl Drop for InstallerMutex {
    fn drop(&mut self) { unsafe { let _ = windows::Win32::Foundation::CloseHandle(self.0); } }
}

fn main() -> Result<()> {
    let _installer_mutex = InstallerMutex(unsafe {
        windows::Win32::System::Threading::CreateMutexW(None, false, windows::core::w!("Local\\VirtualDisplayWorkspace.App"))?
    });
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,virtual_display_workspace=debug,wgpu_core=warn,wgpu_hal=warn".into());
    match diagnostics::log_writer() {
        Ok((writer, path)) => {
            tracing_subscriber::fmt().with_ansi(false).with_env_filter(filter).with_writer(move || writer.clone()).init();
            tracing::info!(version = env!("CARGO_PKG_VERSION"), log_path = %path.display(), "application starting");
        }
        Err(error) => {
            tracing_subscriber::fmt().with_ansi(false).with_env_filter(filter).init();
            tracing::error!(%error, "could not open persistent diagnostic log");
        }
    }
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let mut app = App::new();
    let settings_hwnd = std::sync::Arc::clone(&app.settings_hwnd);
    let mut builder = EventLoop::builder();
    builder.with_msg_hook(move |message| settings::message_hook(&settings_hwnd, message));
    let event_loop = builder.build()?;
    app.settings_proxy = Some(event_loop.create_proxy());
    event_loop.run_app(&mut app)?;
    Ok(())
}
