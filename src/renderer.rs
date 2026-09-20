use anyhow::Result;
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::capture::CapturedFrame;
use crate::viewport::ViewportController;

const SHADER: &str = r#"
struct Viewport { scale_x: f32, scale_y: f32, pan_x: f32, pan_y: f32 }
@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;
@group(0) @binding(2) var<uniform> viewport: Viewport;
struct VertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
  var positions = array<vec2<f32>, 6>(vec2(-1.0,-1.0), vec2(1.0,-1.0), vec2(-1.0,1.0), vec2(-1.0,1.0), vec2(1.0,-1.0), vec2(1.0,1.0));
  var out: VertexOutput;
  out.position = vec4(positions[index], 0.0, 1.0);
  out.uv = positions[index] * 0.5 + vec2(0.5);
  return out;
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let centered = input.uv - vec2(0.5) - vec2(viewport.pan_x, -viewport.pan_y);
    let uv = centered / vec2(viewport.scale_x, viewport.scale_y) + vec2(0.5);
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) { discard; }
  return textureSample(frame_texture, frame_sampler, vec2(uv.x, 1.0 - uv.y));
}
"#;

pub struct Renderer {
    zoom_ui: crate::zoom_ui::ZoomRenderer,
    overlay: crate::ui::OverlayRenderer,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    texture: Option<wgpu::Texture>,
    viewport_buffer: wgpu::Buffer,
    sampler: wgpu::Sampler,
}

impl Renderer {
    pub async fn new(window: &Window) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let target = unsafe { wgpu::SurfaceTargetUnsafe::from_window(window)? };
        let surface = unsafe { instance.create_surface_unsafe(target)? };
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, compatible_surface: Some(&surface), force_fallback_adapter: false }).await.ok_or_else(|| anyhow::anyhow!("no compatible GPU adapter found"))?;
        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor { label: Some("viewer device"), required_features: wgpu::Features::empty(), required_limits: wgpu::Limits::default(), memory_hints: wgpu::MemoryHints::Performance }, None).await?;
        let size = window.inner_size();
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(capabilities.formats[0]);
        let config = wgpu::SurfaceConfiguration { usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format, width: size.width.max(1), height: size.height.max(1), present_mode: wgpu::PresentMode::Fifo, alpha_mode: capabilities.alpha_modes[0], view_formats: vec![], desired_maximum_frame_latency: 2 };
        surface.configure(&device, &config);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("frame shader"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("frame bindings"), entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
        ] });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("frame pipeline layout"), bind_group_layouts: &[&bind_group_layout], push_constant_ranges: &[] });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor { label: Some("frame pipeline"), layout: Some(&pipeline_layout), vertex: wgpu::VertexState { module: &shader, entry_point: "vs_main", buffers: &[], compilation_options: Default::default() }, fragment: Some(wgpu::FragmentState { module: &shader, entry_point: "fs_main", targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }), primitive: Default::default(), depth_stencil: None, multisample: Default::default(), multiview: None, cache: None });
        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("viewport uniform"), contents: bytemuck::cast_slice(&[1.0f32, 1.0, 0.0, 0.0]), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let overlay = crate::ui::OverlayRenderer::new(&device, format);
        Ok(Self { zoom_ui: crate::zoom_ui::ZoomRenderer::new(&device, format), overlay, surface, device, queue, config, pipeline, bind_group_layout, bind_group: None, texture: None, viewport_buffer, sampler })
    }

    pub fn resize(&mut self, width: u32, height: u32) { self.config.width = width.max(1); self.config.height = height.max(1); self.surface.configure(&self.device, &self.config); }

    pub fn update_viewport(&self, viewport: &ViewportController) { self.queue.write_buffer(&self.viewport_buffer, 0, bytemuck::cast_slice(&viewport.normalized_transform())); }

    pub fn update_frame(&mut self, frame: &CapturedFrame) {
        let recreate = self.texture.as_ref().is_none_or(|texture| { let size = texture.size(); size.width != frame.width || size.height != frame.height });
        if recreate {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor { label: Some("desktop frame"), size: wgpu::Extent3d { width: frame.width, height: frame.height, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Bgra8UnormSrgb, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
            let view = texture.create_view(&Default::default());
            self.bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("frame bind group"), layout: &self.bind_group_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }, wgpu::BindGroupEntry { binding: 2, resource: self.viewport_buffer.as_entire_binding() }] }));
            self.texture = Some(texture);
        }
        self.queue.write_texture(wgpu::ImageCopyTexture { texture: self.texture.as_ref().unwrap(), mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &frame.pixels, wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(frame.stride), rows_per_image: Some(frame.height) }, wgpu::Extent3d { width: frame.width, height: frame.height, depth_or_array_layers: 1 });
    }

    pub fn render(&mut self, overlay: &crate::ui::OverlayVisual, zoom: &crate::zoom_ui::ZoomUi, viewport: &ViewportController) -> std::result::Result<(), wgpu::SurfaceError> {
        self.overlay.update(&self.queue, overlay);
        self.zoom_ui.update(&self.device, &self.queue, zoom, viewport);
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("render encoder") });
        { let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some("frame pass"), color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })], depth_stencil_attachment: None, occlusion_query_set: None, timestamp_writes: None }); pass.set_pipeline(&self.pipeline); if let Some(bind_group) = &self.bind_group { pass.set_bind_group(0, bind_group, &[]); pass.draw(0..6, 0..1); } self.overlay.draw(&mut pass); self.zoom_ui.draw(&mut pass); }
        self.queue.submit(Some(encoder.finish())); output.present(); Ok(())
    }
}
