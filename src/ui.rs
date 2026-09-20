use std::time::{Duration, Instant};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, WindowFromPoint};
use wgpu::util::DeviceExt;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton};
use winit::window::Window;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayAction { MoveWindow, Settings, Fullscreen, Minimize, MaximizeRestore, Close }

const ACTIONS: [OverlayAction; 5] = [OverlayAction::Settings, OverlayAction::Fullscreen, OverlayAction::Minimize, OverlayAction::MaximizeRestore, OverlayAction::Close];
const BUTTON_WIDTH: f64 = 46.0;
const BUTTON_HEIGHT: f64 = 32.0;
const DRAG_WIDTH: f64 = 128.0;
const HIDE_DELAY: Duration = Duration::from_millis(350);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OverlayVisual {
    screen: [f32; 4],
    panel: [f32; 4],
    state: [f32; 4],
}

pub struct OverlayState {
    opacity: f32,
    last_tick: Instant,
    last_hover: Option<Instant>,
    hovered: Option<OverlayAction>,
    pressed: Option<OverlayAction>,
}

impl OverlayState {
    pub fn new() -> Self {
        Self { opacity: 0.0, last_tick: Instant::now(), last_hover: None, hovered: None, pressed: None }
    }

    pub fn update(&mut self, window: &Window) -> OverlayVisual {
        let fullscreen = window.fullscreen().is_some();
        let mut visual = self.advance(Instant::now(), cursor_position(window), window.inner_size(), window.scale_factor(), window.is_maximized() || fullscreen);
        visual.state[3] = if fullscreen { 1.0 } else { 0.0 };
        visual
    }

    fn advance(&mut self, now: Instant, cursor: Option<(f64,f64)>, size: PhysicalSize<u32>, scale: f64, maximized: bool) -> OverlayVisual {
        let width = f64::from(size.width) / scale;
        let point = cursor.map(|(x,y)| (x/scale,y/scale));
        let near_top = point.is_some_and(|(x,y)| x >= 0.0 && x < width && y >= 0.0
            && y < if self.last_hover.is_some() { 72.0 } else { 48.0 });
        if near_top { self.last_hover = Some(now); }
        let target = self.last_hover.is_some_and(|last| now.duration_since(last) < HIDE_DELAY);
        let delta = now.saturating_duration_since(self.last_tick).as_secs_f32() / 0.14;
        self.last_tick = now;
        self.opacity = if target { (self.opacity+delta).min(1.0) } else { (self.opacity-delta).max(0.0) };
        if !target && self.opacity == 0.0 { self.last_hover = None; }
        let panel_width = (DRAG_WIDTH + BUTTON_WIDTH * ACTIONS.len() as f64).min(width);
        let drag_width = DRAG_WIDTH.min(panel_width * 0.5);
        let button_panel_width = panel_width - drag_width;
        let panel_left = width - panel_width;
        let panel_top = 0.0;
        self.hovered = if target && self.opacity >= 0.5 {
            point.and_then(|(x,y)| {
                if x < panel_left {
                    return None;
                }
                if x < panel_left + drag_width {
                    return (y >= panel_top && y < BUTTON_HEIGHT && (maximized || y >= 8.0))
                        .then_some(OverlayAction::MoveWindow);
                }
                if x >= width || y < panel_top || y >= BUTTON_HEIGHT { return None; }
                if !maximized && (y < 8.0 || x < 8.0 || x >= width-8.0) { return None; }
                ACTIONS.get(((x-panel_left-drag_width)/(button_panel_width/ACTIONS.len() as f64)) as usize).copied()
            })
        } else { None };
        let index = |action: Option<OverlayAction>| action.and_then(|a| ACTIONS.iter().position(|candidate| *candidate == a)).map_or(-1.0,|i| i as f32);
        OverlayVisual {
            screen: [size.width.max(1) as f32,size.height.max(1) as f32,scale as f32,self.opacity],
            panel: [panel_left as f32,panel_top as f32,panel_width as f32,BUTTON_HEIGHT as f32],
            state: [index(self.hovered),index(self.pressed),if maximized { 1.0 } else { 0.0 },0.0],
        }
    }

    /// Standard button semantics: activate on release over the pressed button.
    pub fn mouse_input(&mut self, state: ElementState, button: MouseButton) -> (bool, Option<OverlayAction>) {
        tracing::debug!(?state, ?button, hovered = ?self.hovered, pressed = ?self.pressed, opacity = self.opacity, "caption mouse input");
        if button != MouseButton::Left { return (false,None); }
        if state == ElementState::Pressed {
            self.pressed = self.hovered;
            if self.pressed == Some(OverlayAction::MoveWindow) { (true,self.pressed) } else { (self.pressed.is_some(),None) }
        } else {
            let pressed = self.pressed.take();
            (pressed.is_some(),pressed.filter(|action| *action != OverlayAction::MoveWindow && Some(*action) == self.hovered))
        }
    }

    pub fn cancel_press(&mut self) { self.pressed = None; }
}

/// Polling also sees hover over the native resize band (non-client mouse moves)
/// and notices when another window covers the viewer.
pub(crate) fn cursor_position(window: &Window) -> Option<(f64,f64)> {
    let RawWindowHandle::Win32(handle) = window.window_handle().ok()?.as_raw() else { return None; };
    let hwnd = HWND(handle.hwnd.get() as _);
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point).ok()?;
        let hit = WindowFromPoint(point);
        if hit != hwnd || !ScreenToClient(hwnd,&mut point).as_bool() { return None; }
    }
    let size = window.inner_size();
    (point.x >= 0 && point.y >= 0 && point.x < size.width as i32 && point.y < size.height as i32)
        .then_some((f64::from(point.x),f64::from(point.y)))
}

pub fn show_error(window: &Window, message: &str, language: crate::language::Language) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK, MB_ICONERROR};
    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::Win32(handle) = handle.as_raw() {
            let message: Vec<u16> = language.error(message).encode_utf16().chain(Some(0)).collect();
            let title: Vec<u16> = language.text("仮想ディスプレイの設定", "Virtual display settings").encode_utf16().chain(Some(0)).collect();
            unsafe { MessageBoxW(Some(HWND(handle.hwnd.get() as _)), PCWSTR(message.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONERROR); }
        }
    }
}

pub struct OverlayRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    buffer: wgpu::Buffer,
}

impl OverlayRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("floating caption controls"), source: wgpu::ShaderSource::Wgsl(include_str!("../ui/caption.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("caption layout"), entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None,
            }],
        });
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("caption state"), contents: &[0; std::mem::size_of::<OverlayVisual>()],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("caption bindings"), layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("caption pipeline layout"), bind_group_layouts: &[&layout], push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("caption pipeline"), layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: "vs_main", buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default() }),
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview: None, cache: None,
        });
        Self { pipeline, bind_group, buffer }
    }

    pub fn update(&self, queue: &wgpu::Queue, visual: &OverlayVisual) {
        queue.write_buffer(&self.buffer,0,bytemuck::bytes_of(visual));
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0,&self.bind_group,&[]);
        pass.draw(0..6,0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn update(state: &mut OverlayState, now: Instant, position: Option<(f64,f64)>, scale: f64) -> OverlayVisual {
        state.advance(now,position,PhysicalSize::new((800.0*scale) as u32,(450.0*scale) as u32),scale,false)
    }
    #[test]
    fn only_top_hover_reveals_and_leaving_fades_out() {
        let mut state=OverlayState::new();
        let now=state.last_tick;
        assert_eq!(update(&mut state,now,Some((400.,200.)),1.).screen[3],0.);
        assert_eq!(update(&mut state,now+Duration::from_millis(200),Some((400.,5.)),1.).screen[3],1.);
        assert_eq!(update(&mut state,now+Duration::from_millis(300),None,1.).screen[3],1.);
        assert_eq!(update(&mut state,now+Duration::from_millis(800),None,1.).screen[3],0.);
        assert_eq!(state.hovered,None);
    }
    #[test]
    fn buttons_use_release_and_cancel_when_pointer_leaves() {
        let mut state=OverlayState::new();
        let now=state.last_tick+Duration::from_secs(1);
        update(&mut state,now,Some((770.,25.)),1.);
        assert_eq!(state.mouse_input(ElementState::Pressed,MouseButton::Left),(true,None));
        update(&mut state,now+Duration::from_millis(10),Some((700.,200.)),1.);
        assert_eq!(state.mouse_input(ElementState::Released,MouseButton::Left),(true,None));
        update(&mut state,now+Duration::from_millis(20),Some((770.,25.)),1.);
        state.mouse_input(ElementState::Pressed,MouseButton::Left);
        assert_eq!(state.mouse_input(ElementState::Released,MouseButton::Left),(true,Some(OverlayAction::Close)));
    }
    #[test]
    fn all_five_buttons_follow_dpi_and_resize_band_is_not_clickable() {
        for scale in [1.,1.5,2.] {
            let mut state=OverlayState::new();
            let now=state.last_tick+Duration::from_secs(1);
            for (x,action) in [(593.,OverlayAction::Settings),(639.,OverlayAction::Fullscreen),(685.,OverlayAction::Minimize),(731.,OverlayAction::MaximizeRestore),(777.,OverlayAction::Close)] {
                update(&mut state,now,Some((x*scale,25.*scale)),scale);
                assert_eq!(state.hovered,Some(action));
            }
            update(&mut state,now,Some((790.*scale,5.*scale)),scale);
            assert_eq!(state.hovered,None);
            assert_eq!(state.mouse_input(ElementState::Pressed,MouseButton::Left),(false,None));
        }
    }
    #[test]
    fn panel_is_flush_with_top_right_even_in_narrow_windows() {
        for width in [180,800,1600] {
            let mut state=OverlayState::new();
            let now=state.last_tick+Duration::from_secs(1);
            let visual=state.advance(now,Some((f64::from(width)-1.0,1.0)),PhysicalSize::new(width,450),1.0,true);
            assert_eq!(visual.panel[1],0.0);
            assert_eq!(visual.panel[0]+visual.panel[2],width as f32);
            assert!(visual.panel[0]>=0.0);
            assert_eq!(state.hovered,Some(OverlayAction::Close));
        }
    }
    #[test]
    fn hidden_controls_and_right_click_never_activate() {
        let mut state=OverlayState::new();
        assert_eq!(state.mouse_input(ElementState::Pressed,MouseButton::Left),(false,None));
        let now=state.last_tick+Duration::from_secs(1);
        update(&mut state,now,Some((770.,25.)),1.);
        assert_eq!(state.mouse_input(ElementState::Pressed,MouseButton::Right),(false,None));
        assert_eq!(state.mouse_input(ElementState::Released,MouseButton::Left),(false,None));
    }

    #[test]
    fn space_left_of_settings_is_a_window_drag_region() {
        let mut state=OverlayState::new();
        let now=state.last_tick+Duration::from_secs(1);
        update(&mut state,now,Some((500.,25.)),1.);
        assert_eq!(state.hovered,Some(OverlayAction::MoveWindow));
        assert_eq!(state.mouse_input(ElementState::Pressed,MouseButton::Left),(true,Some(OverlayAction::MoveWindow)));
        assert_eq!(state.mouse_input(ElementState::Released,MouseButton::Left),(true,None));
    }
}
