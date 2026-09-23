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

struct ViewerWindow {
    window: Window,
    renderer: Renderer,
    viewport: ViewportController,
    overlay: OverlayState,
    zoom_ui: zoom_ui::ZoomUi,
    window_sizing: window_sizing::WindowSizing,
    dragging: bool,
    last_cursor: Vec2,
    space_down: bool,
    fullscreen_restore_size: Option<winit::dpi::PhysicalSize<u32>>,
}

impl ViewerWindow {
    fn toggle_fullscreen(&mut self) {
        let entering = self.window.fullscreen().is_none();
        self.window_sizing.set_fullscreen(entering);
        if entering {
            self.fullscreen_restore_size = (!self.window.is_maximized()).then(|| self.window.inner_size());
            self.window.set_fullscreen(Some(Fullscreen::Borderless(self.window.current_monitor())));
        } else {
            self.window.set_fullscreen(None);
            if let Some(size) = self.fullscreen_restore_size.take() {
                self.window_sizing.restore_size(&self.window, size);
            }
        }
        self.dragging = false;
        tracing::info!(fullscreen = entering, "viewer fullscreen changed");
    }

    fn move_cursor_to_display(&self, cursor: Vec2, bounds: Option<windows::Win32::Foundation::RECT>) {
        let Some(bounds) = bounds else { return };
        let position = self.viewport.content_position_at(cursor);
        let x = bounds.left.saturating_add(position.x.round() as i32);
        let y = bounds.top.saturating_add(position.y.round() as i32);
        unsafe { let _ = SetCursorPos(x, y); }
    }
}

struct App {
    language: language::Language,
    settings_proxy: Option<winit::event_loop::EventLoopProxy<()>>,
    settings: Option<settings::SettingsWindow>,
    settings_hwnd: std::sync::Arc<std::sync::atomic::AtomicIsize>,
    viewers: Vec<ViewerWindow>,
    duplicator: Option<DesktopDuplicator>,
    driver_session: Option<DriverSession>,
    capture_display_name: Option<String>,
    capture_bounds: Option<windows::Win32::Foundation::RECT>,
    next_capture_recovery: std::time::Instant,
    displays: Vec<DisplayInfo>,
    modifiers: ModifiersState,
    content_size: Vec2,
    last_frame: Option<std::sync::Arc<capture::CapturedFrame>>,
    window_counter: usize,
    initialized: bool,
}

impl App {
    fn new() -> Self {
        Self {
            language: language::Language::load(),
            settings_proxy: None,
            settings: None,
            settings_hwnd: std::sync::Arc::new(std::sync::atomic::AtomicIsize::new(0)),
            viewers: Vec::new(),
            duplicator: None,
            driver_session: None,
            capture_display_name: None,
            capture_bounds: None,
            next_capture_recovery: std::time::Instant::now(),
            displays: Vec::new(),
            modifiers: ModifiersState::empty(),
            content_size: Vec2::new(1920.0, 1080.0),
            last_frame: None,
            window_counter: 0,
            initialized: false,
        }
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let mut driver_session = None;
        let mut duplicator = None;
        let mut content_size = Vec2::new(1920.0, 1080.0);
        let mut display_status = None;

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
                    display_status = Some("waiting for virtual display");
                    driver_session = Some(session);
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Parsec VDD is unavailable; starting setup state");
                display_status = Some("Parsec VDD required");
            }
        }

        self.content_size = content_size;
        self.duplicator = duplicator;
        self.driver_session = driver_session;

        self.create_viewer_window(event_loop)?;
        if let Some(status) = display_status {
            if let Some(viewer) = self.viewers.first() {
                viewer.window.set_title(&format!("Virtual Display Workspace - {status}"));
            }
        }
        self.initialized = true;
        Ok(())
    }

    fn create_viewer_window(&mut self, event_loop: &ActiveEventLoop) -> Result<WindowId> {
        self.window_counter += 1;
        let title = if self.window_counter == 1 {
            "Virtual Display Workspace".to_string()
        } else {
            format!("Virtual Display Workspace ({})", self.window_counter)
        };
        let attributes = WindowAttributes::default()
            .with_title(title)
            .with_window_icon(Some(app_icon::load()?))
            .with_decorations(false)
            .with_resizable(true)
            .with_visible(false);
        let window = event_loop.create_window(attributes)?;
        let window_sizing = window_sizing::WindowSizing::new();
        window_sizing.install(&window)?;
        window_sizing.set_content_size(&window, self.content_size.x as u32, self.content_size.y as u32, true);

        if let Some(first) = self.viewers.first() {
            if let Ok(pos) = first.window.outer_position() {
                let offset = ((self.viewers.len() % 8 + 1) * 32) as i32;
                window.set_outer_position(winit::dpi::PhysicalPosition::new(pos.x + offset, pos.y + offset));
            }
        }

        let mut renderer = pollster::block_on(Renderer::new(&window))?;
        if let Some(frame) = self.last_frame.as_ref() {
            renderer.update_frame(frame);
        }

        window.set_visible(true);
        let mut viewport = ViewportController::new(self.content_size);
        let size = window.inner_size();
        viewport.resize(Vec2::new(size.width as f32, size.height as f32));
        renderer.update_viewport(&viewport);

        let mut zoom_ui = zoom_ui::ZoomUi::new();
        zoom_ui.language = self.language;

        let window_id = window.id();
        window.request_redraw();

        self.viewers.push(ViewerWindow {
            window,
            renderer,
            viewport,
            overlay: OverlayState::new(),
            zoom_ui,
            window_sizing,
            dragging: false,
            last_cursor: Vec2::ZERO,
            space_down: false,
            fullscreen_restore_size: None,
        });

        self.render_window(window_id);

        tracing::info!(count = self.viewers.len(), ?window_id, "new viewer window created");
        Ok(window_id)
    }

    fn close_viewer_window(&mut self, window_id: WindowId, event_loop: &ActiveEventLoop) {
        if let Some(pos) = self.viewers.iter().position(|v| v.window.id() == window_id) {
            self.viewers.remove(pos);
            tracing::info!(remaining = self.viewers.len(), ?window_id, "viewer window closed");
        }
        if self.viewers.is_empty() {
            tracing::info!("all viewer windows closed; exiting application");
            event_loop.exit();
        }
    }

    fn update_capture(&mut self) -> bool {
        let mut capture_lost = self.duplicator.is_none();
        let mut updated = false;
        if let Some(duplicator) = self.duplicator.as_mut() {
            match duplicator.next_frame(std::time::Duration::from_millis(1)) {
                Ok(Some(frame)) => {
                    if frame.frame_id == 1 {
                        tracing::debug!(width = frame.width, height = frame.height, "uploading first frame to renderers");
                    }
                    let frame = std::sync::Arc::new(frame);
                    self.last_frame = Some(std::sync::Arc::clone(&frame));
                    for viewer in &mut self.viewers {
                        viewer.renderer.update_frame(&frame);
                        viewer.renderer.update_viewport(&viewer.viewport);
                    }
                    updated = true;
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, "desktop duplication lost; scheduling recovery");
                    capture_lost = true;
                }
            }
        }
        if capture_lost {
            drop(self.duplicator.take());
            self.recreate_capture();
        }
        updated
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
                self.content_size = Vec2::new(selected_display.width.max(1) as f32, selected_display.height.max(1) as f32);
                for viewer in &mut self.viewers {
                    viewer.window_sizing.set_content_size(&viewer.window, selected_display.width, selected_display.height, false);
                    viewer.viewport.content_size = self.content_size;
                    viewer.viewport.resize(viewer.viewport.viewport_size);
                }
            }
            Err(error) => tracing::warn!(error = %format!("{error:#}"), "failed to recreate desktop duplication"),
        }
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
                    if let Some(mode) = view {
                        for viewer in &mut self.viewers {
                            viewer.viewport.reset(mode);
                        }
                    }
                    if language != self.language {
                        if let Err(error) = language.save() {
                            if let Some(settings) = &self.settings { settings.set_status(&format!("{}: {error}", self.language.text("言語を保存できませんでした", "Could not save the language"))); }
                            continue;
                        }
                        self.language = language;
                        for viewer in &mut self.viewers {
                            viewer.zoom_ui.language = language;
                        }
                    }
                    self.settings = None;
                }
            }
        }
    }

    fn handle_overlay(&mut self, window_id: WindowId, action: OverlayAction, event_loop: &ActiveEventLoop) {
        tracing::debug!(?action, ?window_id, "caption action invoked");
        match action {
            OverlayAction::MoveWindow => {}
            OverlayAction::NewWindow => {
                if let Err(error) = self.create_viewer_window(event_loop) {
                    tracing::error!(%error, "failed to create new viewer window");
                }
            }
            OverlayAction::Settings => {
                let owner = self.viewers.iter().find(|v| v.window.id() == window_id)
                    .map(|v| &v.window)
                    .or_else(|| self.viewers.first().map(|v| &v.window));
                let Some(owner) = owner else { return };
                if self.settings.is_none() {
                    match settings::SettingsWindow::new(event_loop, owner, std::sync::Arc::clone(&self.settings_hwnd), self.settings_proxy.as_ref().unwrap().clone(), self.language) {
                        Ok(settings) => { self.settings = Some(settings); self.refresh_settings(); },
                        Err(error) => { ui::show_error(owner, &error.to_string(), self.language); return; }
                    }
                }
                if let Some(settings) = &self.settings { settings.show(); }
            }
            OverlayAction::Fullscreen => {
                if let Some(viewer) = self.viewers.iter_mut().find(|v| v.window.id() == window_id) {
                    viewer.toggle_fullscreen();
                }
            }
            OverlayAction::Minimize => {
                if let Some(viewer) = self.viewers.iter().find(|v| v.window.id() == window_id) {
                    viewer.window.set_minimized(true);
                }
            }
            OverlayAction::MaximizeRestore => {
                if let Some(viewer) = self.viewers.iter_mut().find(|v| v.window.id() == window_id) {
                    if viewer.window.fullscreen().is_some() {
                        viewer.toggle_fullscreen();
                    } else {
                        viewer.window.set_maximized(!viewer.window.is_maximized());
                    }
                }
            }
            OverlayAction::Close => {
                self.close_viewer_window(window_id, event_loop);
            }
        }
    }

    fn render_window(&mut self, window_id: WindowId) {
        let language = self.language;
        let Some(viewer) = self.viewers.iter_mut().find(|v| v.window.id() == window_id) else { return };
        let window_size = viewer.window.inner_size();
        viewer.zoom_ui.language = language;
        viewer.zoom_ui.update(&viewer.window, &mut viewer.viewport);
        let overlay_visual = viewer.overlay.update(&viewer.window);
        viewer.renderer.update_viewport(&viewer.viewport);
        match viewer.renderer.render(&overlay_visual, &viewer.zoom_ui, &viewer.viewport) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                viewer.renderer.resize(window_size.width, window_size.height);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => std::process::exit(1),
            Err(wgpu::SurfaceError::Timeout) => {}
        }
        viewer.window.request_redraw();
    }

    fn render_all(&mut self) {
        let ids: Vec<WindowId> = self.viewers.iter().map(|v| v.window.id()).collect();
        for id in ids {
            self.render_window(id);
        }
    }

    fn redraw_window(&mut self, window_id: WindowId) {
        if self.update_capture() {
            self.render_all();
        } else {
            self.render_window(window_id);
        }
    }
}

impl ApplicationHandler for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, (): ()) { self.process_settings(); }
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        self.process_settings();
        if self.update_capture() {
            self.render_all();
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.initialized {
            if let Err(error) = self.initialize(event_loop) {
                tracing::error!(%error, "failed to initialize viewer");
                event_loop.exit();
            }
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

        if !self.viewers.iter().any(|v| v.window.id() == window_id) { return; }

        match event {
            WindowEvent::CloseRequested => {
                self.close_viewer_window(window_id, event_loop);
            }
            WindowEvent::RedrawRequested => {
                self.redraw_window(window_id);
            }
            WindowEvent::Resized(size) => {
                if let Some(viewer) = self.viewers.iter_mut().find(|v| v.window.id() == window_id) {
                    viewer.renderer.resize(size.width, size.height);
                    viewer.viewport.resize(Vec2::new(size.width as f32, size.height as f32));
                }
            }
            WindowEvent::Focused(false) => {
                if let Some(viewer) = self.viewers.iter_mut().find(|v| v.window.id() == window_id) {
                    viewer.overlay.cancel_press();
                    viewer.zoom_ui.cancel();
                    viewer.dragging = false;
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event: KeyEvent { physical_key: PhysicalKey::Code(code), state, repeat, .. }, .. } => {
                let pressed = state == ElementState::Pressed;
                if pressed && self.modifiers.control_key() && code == KeyCode::KeyN {
                    let _ = self.create_viewer_window(event_loop);
                    return;
                }
                if let Some(viewer) = self.viewers.iter_mut().find(|v| v.window.id() == window_id) {
                    if pressed && code == KeyCode::Escape && viewer.zoom_ui.escape() { return; }
                    if pressed && !repeat && (code == KeyCode::F11 || (code == KeyCode::Escape && viewer.window.fullscreen().is_some())) {
                        viewer.toggle_fullscreen();
                    }
                    if code == KeyCode::Space { viewer.space_down = pressed; }
                    if pressed && self.modifiers.control_key() && code == KeyCode::Digit0 {
                        viewer.viewport.reset(if self.modifiers.shift_key() { ResetMode::ActualSize } else { ResetMode::Fit });
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let mut overlay_action = None;
                let mut drag_window = false;
                let mut move_cursor = false;
                let capture_bounds = self.capture_bounds;

                if let Some(viewer) = self.viewers.iter_mut().find(|v| v.window.id() == window_id) {
                    viewer.zoom_ui.update(&viewer.window, &mut viewer.viewport);
                    if viewer.zoom_ui.mouse(state, button, &mut viewer.viewport) {
                        viewer.dragging = false;
                        return;
                    }
                    viewer.overlay.update(&viewer.window);
                    let (consumed, action) = viewer.overlay.mouse_input(state, button);
                    if consumed {
                        viewer.dragging = false;
                        if let Some(action) = action {
                            if action == OverlayAction::MoveWindow {
                                drag_window = true;
                            } else {
                                overlay_action = Some(action);
                            }
                        }
                    } else if state == ElementState::Pressed && button == MouseButton::Left && self.modifiers.control_key() {
                        move_cursor = true;
                        viewer.dragging = false;
                    } else {
                        let zoomed = viewer.viewport.zoom > 1.0;
                        viewer.dragging = state == ElementState::Pressed
                            && (button == MouseButton::Middle || (button == MouseButton::Left && (viewer.space_down || zoomed)));
                    }
                }

                if drag_window {
                    if let Some(viewer) = self.viewers.iter().find(|v| v.window.id() == window_id) {
                        let _ = viewer.window.drag_window();
                    }
                }
                if move_cursor {
                    if let Some(viewer) = self.viewers.iter().find(|v| v.window.id() == window_id) {
                        viewer.move_cursor_to_display(viewer.last_cursor, capture_bounds);
                    }
                }
                if let Some(action) = overlay_action {
                    self.handle_overlay(window_id, action, event_loop);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor = Vec2::new(position.x as f32, position.y as f32);
                if let Some(viewer) = self.viewers.iter_mut().find(|v| v.window.id() == window_id) {
                    if viewer.zoom_ui.cursor_moved(cursor, &mut viewer.viewport) {
                        viewer.last_cursor = cursor;
                        return;
                    }
                    if viewer.dragging {
                        viewer.viewport.pan_by(cursor - viewer.last_cursor);
                    }
                    viewer.last_cursor = cursor;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(viewer) = self.viewers.iter_mut().find(|v| v.window.id() == window_id) {
                    viewer.zoom_ui.update(&viewer.window, &mut viewer.viewport);
                    let amount = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32,
                    };
                    if viewer.zoom_ui.wheel(amount, &mut viewer.viewport) { return; }
                    let movement = match delta {
                        MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y) * 48.0,
                        MouseScrollDelta::PixelDelta(position) => Vec2::new(position.x as f32, position.y as f32),
                    };
                    if self.modifiers.control_key() {
                        let amount = match delta {
                            MouseScrollDelta::LineDelta(_, y) => y,
                            MouseScrollDelta::PixelDelta(position) => position.y as f32 / 100.0,
                        };
                        viewer.viewport.zoom_at(amount, viewer.last_cursor);
                    } else {
                        viewer.viewport.pan_by(movement);
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
            tracing_subscriber::fmt().with_ansi(false).with_env_filter(filter)
                .with_writer(move || diagnostics::DiagnosticWriter::new(writer.clone())).init();
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
