//! Floating zoom controls. GDI rasterizes DPI-aware text; wgpu composites the panel.
use std::time::{Duration, Instant};
use glam::Vec2;
use winit::{event::{ElementState, MouseButton}, window::Window};
use crate::viewport::ViewportController;

const LABELS: [&str; 10] = ["100%", "150%", "200%", "300%", "400%", "実寸（ピクセル等倍）", "実寸 1.5倍", "実寸 2倍", "実寸 3倍", "実寸 4倍"];
pub fn choices(viewport: &ViewportController) -> [Option<f32>; 10] {
    [Some(1.), Some(1.5), Some(2.), Some(3.), Some(4.), viewport.actual_zoom(1.),
        viewport.actual_zoom(1.5), viewport.actual_zoom(2.), viewport.actual_zoom(3.), viewport.actual_zoom(4.)]
}

#[derive(Clone, Copy, Default)]
struct Layout { x: f32, y: f32, width: f32, rows: usize }
impl Layout {
    fn new(size: Vec2) -> Self {
        let width = 360_f32.min((size.x-32.).max(120.));
        Self { x: size.x-width-16., y: size.y-60., width, rows: (((size.y-84.)/28.).floor() as usize).clamp(2,10) }
    }
    fn slider(&self) -> (f32, f32) { (self.x+68., self.x+self.width-116.) }
    fn menu_y(&self) -> f32 { self.y-8.-self.rows as f32*28.-12. }
    fn menu_width(&self) -> f32 { 244_f32.min(self.width) }
    fn in_bar(&self, p: Vec2) -> bool { p.x>=self.x && p.x<self.x+self.width && p.y>=self.y && p.y<self.y+44. }
    fn in_menu(&self, p: Vec2) -> bool { p.x>=self.x+self.width-self.menu_width() && p.x<self.x+self.width && p.y>=self.menu_y() && p.y<self.y-8. }
    fn zoom_at(&self, x: f32) -> f32 { let (a,b)=self.slider(); 1.+3.*((x-a)/(b-a).max(1.)).clamp(0.,1.) }
}

pub struct ZoomUi {
    pub language: crate::language::Language,
    opacity: f32, last_tick: Instant, last_hover: Option<Instant>,
    layout: Layout, cursor: Option<Vec2>, dragging: bool, pressed: bool,
    open: bool, scroll: usize, hover: Option<usize>, scale: f32,
}
impl ZoomUi {
    pub fn new() -> Self { Self { language: crate::language::Language::Japanese, opacity: 0., last_tick: Instant::now(), last_hover: None, layout: Layout::default(), cursor: None,
        dragging: false, pressed: false, open: false, scroll: 0, hover: None, scale: 1. } }

    pub fn update(&mut self, window: &Window, viewport: &mut ViewportController) {
        self.scale=window.scale_factor() as f32;
        // Keep the three controls usable together in narrow portrait viewers.
        self.scale*=((window.inner_size().width as f32/self.scale-32.)/240.).clamp(0.25,1.);
        self.layout=Layout::new(Vec2::new(window.inner_size().width as f32, window.inner_size().height as f32)/self.scale);
        self.scroll=self.scroll.min(10-self.layout.rows);
        self.cursor=crate::ui::cursor_position(window).map(|(x,y)|Vec2::new(x as f32,y as f32)/self.scale);
        let near = self.cursor.is_some_and(|p| p.x>=self.layout.x-12. && p.y>=self.layout.y-12.);
        let now=Instant::now();
        if near || self.dragging || self.open { self.last_hover=Some(now); }
        let visible=self.last_hover.is_some_and(|t|now.duration_since(t)<Duration::from_millis(500));
        let delta=now.duration_since(self.last_tick).as_secs_f32()/0.16;
        self.last_tick=now;
        self.opacity=if visible {(self.opacity+delta).min(1.)}else{(self.opacity-delta).max(0.)};
        self.hover=self.cursor.and_then(|p| {
            if self.open && self.layout.in_menu(p) {
                let row=((p.y-self.layout.menu_y()-6.)/28.).floor() as isize;
                if row>=0 && row<(self.layout.rows as isize) { return Some(self.scroll+row as usize); }
            }
            None
        });
        if self.dragging { if let Some(p)=self.cursor { viewport.set_zoom(self.layout.zoom_at(p.x)); } }
    }

    pub fn mouse(&mut self, state: ElementState, button: MouseButton, viewport: &mut ViewportController) -> bool {
        if button!=MouseButton::Left { return self.open || self.cursor.is_some_and(|p|self.opacity>0.5 && self.layout.in_bar(p)); }
        if state==ElementState::Released {
            let consumed=self.pressed;
            self.pressed=false; self.dragging=false;
            return consumed;
        }
        let Some(p)=self.cursor else { let was=self.open; self.open=false; return was; };
        if self.open && self.layout.in_menu(p) {
            if let Some(index)=self.hover { if let Some(zoom)=choices(viewport)[index] {
                viewport.set_zoom(zoom); self.open=false;
                tracing::debug!(index, zoom, "zoom preset selected");
            } }
            self.pressed=true; return true;
        }
        if self.opacity>0.5 && self.layout.in_bar(p) {
            self.pressed=true;
            if p.x>=self.layout.x+self.layout.width-100. { self.open = !self.open; }
            else if p.x>=self.layout.x+60. {
                self.dragging=true; self.open=false; viewport.set_zoom(self.layout.zoom_at(p.x));
            }
            return true;
        }
        let was=self.open; self.open=false; self.pressed=was; was
    }

    pub fn cursor_moved(&mut self, position: Vec2, viewport: &mut ViewportController) -> bool {
        if self.dragging { viewport.set_zoom(self.layout.zoom_at(position.x/self.scale)); true } else { false }
    }
    pub fn wheel(&mut self, delta: f32, viewport: &mut ViewportController) -> bool {
        if self.open {
            self.scroll=(self.scroll as i32 - delta.signum() as i32).clamp(0,(10-self.layout.rows) as i32) as usize;
            return true;
        }
        if self.opacity>0.5 && self.cursor.is_some_and(|p|self.layout.in_bar(p)) {
            viewport.set_zoom(viewport.zoom+delta.signum()*0.05); return true;
        }
        false
    }
    pub fn escape(&mut self) -> bool { let was=self.open || self.dragging; self.cancel(); was }
    pub fn cancel(&mut self) { self.open=false; self.dragging=false; self.pressed=false; }
}

use windows::{core::w, Win32::{Foundation::{COLORREF, RECT, SIZE}, Graphics::Gdi::*}};
use wgpu::util::DeviceExt;

struct Canvas { dc: HDC, bitmap: HBITMAP, old_bitmap: HGDIOBJ, font: HFONT, latin_font: HFONT, old_font: HGDIOBJ, bits: *mut u8, width: u32, height: u32, scale: f32 }
impl Canvas {
    fn new(width: u32, height: u32, scale: f32) -> anyhow::Result<Self> {
        unsafe {
            let dc=CreateCompatibleDC(None);
            anyhow::ensure!(!dc.is_invalid(), "zoom UI device context creation failed");
            let info=BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: width as i32, biHeight: -(height as i32), biPlanes:1, biBitCount:32, biCompression:BI_RGB.0, ..Default::default() }, ..Default::default() };
            let mut bits=std::ptr::null_mut();
            let bitmap=match CreateDIBSection(Some(dc),&info,DIB_RGB_COLORS,&mut bits,None,0) { Ok(b)=>b, Err(e)=>{let _=DeleteDC(dc);return Err(e.into());} };
            let old_bitmap=SelectObject(dc,bitmap.into());
            let font=CreateFontW(-(13.*scale).round() as i32,0,0,0,400,0,0,0,DEFAULT_CHARSET,OUT_DEFAULT_PRECIS,CLIP_DEFAULT_PRECIS,ANTIALIASED_QUALITY,0,w!("Yu Gothic UI"));
            let latin_font=CreateFontW(-(13.*scale).round() as i32,0,0,0,400,0,0,0,DEFAULT_CHARSET,OUT_DEFAULT_PRECIS,CLIP_DEFAULT_PRECIS,ANTIALIASED_QUALITY,0,w!("Segoe UI"));
            let old_font=SelectObject(dc,font.into());
            let _=SetBkMode(dc,TRANSPARENT);
            std::ptr::write_bytes(bits,0,(width*height*4) as usize);
            Ok(Self {dc,bitmap,old_bitmap,font,latin_font,old_font,bits:bits.cast(),width,height,scale})
        }
    }
    fn rect(&self, r:[f32;4]) -> RECT { RECT {left:(r[0]*self.scale).round() as i32,top:(r[1]*self.scale).round() as i32,right:((r[0]+r[2])*self.scale).round() as i32,bottom:((r[1]+r[3])*self.scale).round() as i32} }
    fn rounded(&self,r:[f32;4],radius:f32,color:u32) { unsafe {
        let rect=self.rect(r); let brush=CreateSolidBrush(COLORREF(color));
        let old_brush=SelectObject(self.dc,brush.into()); let old_pen=SelectObject(self.dc,GetStockObject(NULL_PEN));
        let _=RoundRect(self.dc,rect.left,rect.top,rect.right,rect.bottom,(radius*2.*self.scale) as i32,(radius*2.*self.scale) as i32);
        SelectObject(self.dc,old_brush); SelectObject(self.dc,old_pen); let _=DeleteObject(brush.into());
    } }
    fn text(&self,text:&str,r:[f32;4],color:u32) { unsafe {
        // Explicit script runs avoid Windows choosing an unrelated Japanese fallback.
        let rect=self.rect(r);
        let saved=SaveDC(self.dc);
        IntersectClipRect(self.dc,rect.left,rect.top,rect.right,rect.bottom);
        SetTextColor(self.dc,COLORREF(color));
        let mut runs:Vec<(bool,String)>=Vec::new();
        for ch in text.chars() {
            let latin=(ch as u32)<=0x024f;
            if let Some((previous,run))=runs.last_mut().filter(|(previous,_)|*previous==latin) { let _=previous;run.push(ch); }
            else {runs.push((latin,ch.to_string()));}
        }
        let mut measured=Vec::new();let mut ascent=0;let mut descent=0;
        for (latin,run) in runs {
            let font=if latin {self.latin_font}else{self.font};SelectObject(self.dc,font.into());
            let mut metrics=TEXTMETRICW::default();let _=GetTextMetricsW(self.dc,&mut metrics);
            let run:Vec<u16>=run.encode_utf16().collect();let mut size=SIZE::default();let _=GetTextExtentPoint32W(self.dc,&run,&mut size);
            ascent=ascent.max(metrics.tmAscent);descent=descent.max(metrics.tmDescent);
            measured.push((font,run,metrics.tmAscent,size.cx));
        }
        let baseline=rect.top+(rect.bottom-rect.top-ascent-descent)/2+ascent;let mut x=rect.left;
        for (font,run,ascent,width) in measured {SelectObject(self.dc,font.into());let _=TextOutW(self.dc,x,baseline-ascent,&run);x+=width;}
        let _=RestoreDC(self.dc,saved);
    } }
    fn circle(&self, center:[f32;2], radius:f32, color:u32) { unsafe {
        // Analytic coverage at physical-pixel centers gives a smooth round thumb at any DPI.
        let _=GdiFlush();
        let cx=center[0]*self.scale;let cy=center[1]*self.scale;let radius=radius*self.scale;
        let pixels=std::slice::from_raw_parts_mut(self.bits,(self.width*self.height*4) as usize);
        for y in ((cy-radius-1.).floor().max(0.) as u32)..((cy+radius+1.).ceil() as u32).min(self.height) {
            for x in ((cx-radius-1.).floor().max(0.) as u32)..((cx+radius+1.).ceil() as u32).min(self.width) {
                let coverage=(radius+0.5-((x as f32+0.5-cx).powi(2)+(y as f32+0.5-cy).powi(2)).sqrt()).clamp(0.,1.);
                let offset=((y*self.width+x)*4) as usize;
                for (channel,shift) in [16,8,0].into_iter().enumerate() {
                    let value=((color>>shift)&255) as f32;
                    pixels[offset+channel]=(pixels[offset+channel] as f32*(1.-coverage)+value*coverage).round() as u8;
                }
            }
        }
    } }
    fn pixels(&self)->Vec<u8> { unsafe {
        let _=GdiFlush(); let mut pixels=std::slice::from_raw_parts(self.bits,(self.width*self.height*4) as usize).to_vec();
        for pixel in pixels.as_chunks_mut::<4>().0 { pixel[3]=if pixel[0]|pixel[1]|pixel[2]!=0 {255}else{0}; }
        pixels
    } }
}
impl Drop for Canvas { fn drop(&mut self) { unsafe { SelectObject(self.dc,self.old_font); SelectObject(self.dc,self.old_bitmap); let _=DeleteObject(self.font.into()); let _=DeleteObject(self.latin_font.into()); let _=DeleteObject(self.bitmap.into()); let _=DeleteDC(self.dc); } } }

pub struct ZoomRenderer {
    pipeline:wgpu::RenderPipeline, layout:wgpu::BindGroupLayout, uniform:wgpu::Buffer, sampler:wgpu::Sampler,
    texture:Option<wgpu::Texture>, bindings:Option<wgpu::BindGroup>, key:String,
}
impl ZoomRenderer {
    pub fn new(device:&wgpu::Device,format:wgpu::TextureFormat)->Self {
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("zoom panel shader"),source:wgpu::ShaderSource::Wgsl(include_str!("../ui/zoom.wgsl").into())});
        let layout=device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {label:Some("zoom panel layout"),entries:&[
            wgpu::BindGroupLayoutEntry {binding:0,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Texture {sample_type:wgpu::TextureSampleType::Float {filterable:true},view_dimension:wgpu::TextureViewDimension::D2,multisampled:false},count:None},
            wgpu::BindGroupLayoutEntry {binding:1,visibility:wgpu::ShaderStages::FRAGMENT,ty:wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),count:None},
            wgpu::BindGroupLayoutEntry {binding:2,visibility:wgpu::ShaderStages::VERTEX_FRAGMENT,ty:wgpu::BindingType::Buffer {ty:wgpu::BufferBindingType::Uniform,has_dynamic_offset:false,min_binding_size:None},count:None},
        ]});
        let pipeline_layout=device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {label:None,bind_group_layouts:&[&layout],push_constant_ranges:&[]});
        let pipeline=device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {label:Some("zoom panel"),layout:Some(&pipeline_layout),
            vertex:wgpu::VertexState {module:&shader,entry_point:"vs_main",buffers:&[],compilation_options:Default::default()},
            fragment:Some(wgpu::FragmentState {module:&shader,entry_point:"fs_main",targets:&[Some(wgpu::ColorTargetState {format,blend:Some(wgpu::BlendState::ALPHA_BLENDING),write_mask:wgpu::ColorWrites::ALL})],compilation_options:Default::default()}),
            primitive:Default::default(),depth_stencil:None,multisample:Default::default(),multiview:None,cache:None});
        let uniform=device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:None,contents:&[0;32],usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST});
        let sampler=device.create_sampler(&wgpu::SamplerDescriptor {mag_filter:wgpu::FilterMode::Linear,min_filter:wgpu::FilterMode::Linear,..Default::default()});
        Self {pipeline,layout,uniform,sampler,texture:None,bindings:None,key:String::new()}
    }

    pub fn update(&mut self,device:&wgpu::Device,queue:&wgpu::Queue,ui:&ZoomUi,viewport:&ViewportController) {
        let l=ui.layout;
        let top=if ui.open {l.menu_y()}else{l.y};
        let height=l.y+44.-top;
        let width=(l.width*ui.scale).ceil() as u32;
        let pixel_height=(height*ui.scale).ceil() as u32;
        let data=[viewport.viewport_size.x,viewport.viewport_size.y,ui.opacity,0.,l.x*ui.scale,top*ui.scale,width as f32,pixel_height as f32];
        queue.write_buffer(&self.uniform,0,bytemuck::cast_slice(&data));
        if ui.opacity==0. {return;}
        let options=choices(viewport);
        let dropdown_hover=ui.cursor.is_some_and(|p| l.in_bar(p) && p.x>=l.x+l.width-100.);
        let key=format!("{width}:{pixel_height}:{:.3}:{:.3}:{}:{}:{:?}:{}:{:?}",ui.scale,viewport.zoom,ui.open,ui.scroll,ui.hover,dropdown_hover,(options,ui.language));
        if self.key==key {return;}
        let Ok(canvas)=Canvas::new(width,pixel_height,ui.scale) else {return;};
        let y=l.y-top;
        canvas.rounded([0.,y,l.width,44.],12.,0x39332e);
        canvas.rounded([1.,y+1.,l.width-2.,42.],11.,0x272320);
        canvas.text(&format!("{:.0}%",viewport.zoom*100.),[16.,y,52.,44.],0xf5f2ed);
        let (a,b)=l.slider(); let a=a-l.x; let b=b-l.x;
        let knob=a+(b-a)*(viewport.zoom-1.)/3.;
        canvas.rounded([a,y+20.,b-a,4.],2.,0x62564c);
        canvas.rounded([a,y+20.,(knob-a).max(2.),4.],2.,0xe8be6b);
        canvas.circle([knob,y+22.],7.,0xf9f4e8);
        canvas.circle([knob,y+22.],3.5,0xe8be6b);
        canvas.rounded([l.width-100.,y+6.,92.,32.],7.,if dropdown_hover || ui.open {0x554536}else{0x3b322a});
        canvas.text(ui.language.text("倍率", "Zoom"),[l.width-86.,y+6.,56.,32.],0xf5f2ed);
        canvas.text(if ui.open {"⌃"}else{"⌄"},[l.width-30.,y+4.,20.,32.],0xddd0c1);
        if ui.open {
            let x=l.width-l.menu_width();
            canvas.rounded([x,0.,l.menu_width(),l.rows as f32*28.+12.],10.,0x39332e);
            canvas.rounded([x+1.,1.,l.menu_width()-2.,l.rows as f32*28.+10.],9.,0x272320);
            for row in 0..l.rows {
                let i=row+ui.scroll; let row_y=6.+row as f32*28.;
                if ui.hover==Some(i) && options[i].is_some() {canvas.rounded([x+5.,row_y,l.menu_width()-10.,28.],5.,0x554536);}
                let checked=options[i].is_some_and(|zoom|(zoom-viewport.zoom).abs()<0.005);
                if checked {canvas.text("✓",[x+10.,row_y,20.,28.],0xe8be6b);}
                canvas.text(ui.language.text(LABELS[i], ["100%", "150%", "200%", "300%", "400%", "Actual size (1:1)", "Actual size ×1.5", "Actual size ×2", "Actual size ×3", "Actual size ×4"][i]),[x+32.,row_y,l.menu_width()-40.,28.],if options[i].is_some(){0xf5f2ed}else{0x817971});
            }
            if l.rows<10 {
                let track=l.rows as f32*28.; let thumb=track*l.rows as f32/10.; let scroll_y=6.+(track-thumb)*ui.scroll as f32/(10-l.rows) as f32;
                canvas.rounded([l.width-5.,scroll_y,2.,thumb],1.,0xa08a75);
            }
        }
        let pixels=canvas.pixels();
        let recreate=self.texture.as_ref().is_none_or(|t|t.width()!=width || t.height()!=pixel_height);
        if recreate {
            let texture=device.create_texture(&wgpu::TextureDescriptor {label:Some("zoom panel image"),size:wgpu::Extent3d {width,height:pixel_height,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Bgra8UnormSrgb,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
            self.bindings=Some(device.create_bind_group(&wgpu::BindGroupDescriptor {label:None,layout:&self.layout,entries:&[
                wgpu::BindGroupEntry {binding:0,resource:wgpu::BindingResource::TextureView(&texture.create_view(&Default::default()))},
                wgpu::BindGroupEntry {binding:1,resource:wgpu::BindingResource::Sampler(&self.sampler)},wgpu::BindGroupEntry {binding:2,resource:self.uniform.as_entire_binding()},
            ]}));
            self.texture=Some(texture);
        }
        queue.write_texture(wgpu::ImageCopyTexture {texture:self.texture.as_ref().unwrap(),mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},&pixels,wgpu::ImageDataLayout {offset:0,bytes_per_row:Some(width*4),rows_per_image:Some(pixel_height)},wgpu::Extent3d {width,height:pixel_height,depth_or_array_layers:1});
        self.key=key;
    }
    pub fn draw<'a>(&'a self,pass:&mut wgpu::RenderPass<'a>) {if let Some(bindings)=&self.bindings {pass.set_pipeline(&self.pipeline);pass.set_bind_group(0,bindings,&[]);pass.draw(0..6,0..1);}}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn actual_presets_follow_fit_scale_and_bounds() {
        let mut v=ViewportController::new(Vec2::new(1920.,1080.));
        v.resize(Vec2::new(960.,540.));
        assert_eq!(&choices(&v)[5..],&[Some(2.),Some(3.),Some(4.),None,None]);
        v.resize(Vec2::new(3840.,2160.));
        assert_eq!(&choices(&v)[5..],&[None,None,Some(1.),Some(1.5),Some(2.)]);
        v.resize(Vec2::new(384.,216.));
        assert!(choices(&v)[5..].iter().all(Option::is_none));
    }
    #[test] fn slider_clamps_and_menu_fits_short_windows() {
        let l=Layout::new(Vec2::new(800.,450.)); let (a,b)=l.slider();
        assert_eq!(l.zoom_at(a-50.),1.);assert_eq!(l.zoom_at(b+50.),4.);
        assert_eq!(l.zoom_at((a+b)/2.),2.5);
        let short=Layout::new(Vec2::new(320.,180.));assert!(short.menu_y()>=0.);assert!(short.rows<10);
    }
    #[test] fn disabled_menu_item_does_not_change_zoom() {
        let mut v=ViewportController::new(Vec2::new(1920.,1080.));v.resize(Vec2::new(960.,540.));
        let mut ui=ZoomUi::new();ui.layout=Layout::new(Vec2::new(960.,540.));ui.open=true;ui.opacity=1.;ui.hover=Some(9);
        ui.cursor=Some(Vec2::new(ui.layout.x+ui.layout.width-20.,ui.layout.menu_y()+20.));
        assert!(ui.mouse(ElementState::Pressed,MouseButton::Left,&mut v));assert_eq!(v.zoom,1.);assert!(ui.open);
    }
}
