struct Overlay {
    screen: vec4<f32>, // physical width, height, DPI scale, opacity
    panel: vec4<f32>,  // logical x, y, width, height
    state: vec4<f32>,  // hovered index, pressed index, maximized, fullscreen
}
@group(0) @binding(0) var<uniform> overlay: Overlay;
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
}
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var corners = array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
    let local = corners[index] * (overlay.panel.zw + vec2(16.)) - vec2(8.);
    let pixel = (overlay.panel.xy + local) * overlay.screen.z;
    var out: VertexOutput;
    out.position = vec4(pixel.x / overlay.screen.x * 2. - 1., 1. - pixel.y / overlay.screen.y * 2., 0., 1.);
    out.local = local;
    return out;
}
fn rounded_box(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2(radius);
    return length(max(q,vec2(0.))) + min(max(q.x,q.y),0.) - radius;
}
fn square_outline(p: vec2<f32>, size: f32, aa: f32) -> f32 {
    let distance = abs(max(abs(p.x),abs(p.y)) - size);
    return 1. - smoothstep(0.45,0.45+aa,distance);
}
@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if overlay.screen.w <= 0.001 { discard; }
    let p = input.local;
    let aa = 0.7 / overlay.screen.z;
    let distance = rounded_box(p-overlay.panel.zw*0.5,overlay.panel.zw*0.5,0.);
    let coverage = 1. - smoothstep(0.,aa,distance);
    if coverage < 0.001 {
        let shadow_distance = rounded_box(p-overlay.panel.zw*0.5-vec2(0.,2.),overlay.panel.zw*0.5,0.);
        return vec4(0.,0.,0.,0.24 * (1.-smoothstep(0.,7.,max(shadow_distance,0.))) * overlay.screen.w);
    }
    let drag_width = min(128.,overlay.panel.z * 0.5);
    let button_width = (overlay.panel.z-drag_width) / 5.;
    let in_button_area = p.x >= drag_width;
    let index = clamp(floor((p.x-drag_width) / button_width),0.,4.);
    var color = vec3(0.018,0.020,0.025);
    if in_button_area && index == overlay.state.x {
        color = select(vec3(0.065),vec3(0.807,0.006,0.017),index == 4.);
        if index == overlay.state.y { color *= 0.72; }
    }
    let q = p - vec2(drag_width+(index+0.5)*button_width,16.);
    var icon = 0.;
    if index == 0. {
        let radius = length(q);
        let angle = atan2(q.y,q.x);
        let outer = 5.4 + 1.6 * clamp((cos(angle*8.)-0.1)*2.,0.,1.);
        icon = smoothstep(2.1,2.1+aa,radius) * (1.-smoothstep(outer-aa,outer,radius));
    } else if index == 1. {
        let a = abs(q);
        let corner = select(6.,2.5,overlay.state.w > 0.5);
        let line = min(abs(a.x-corner),abs(a.y-corner));
        icon = (1.-smoothstep(0.45,0.45+aa,line)) * smoothstep(2.,2.+aa,min(a.x,a.y)) * (1.-smoothstep(6.,6.+aa,max(a.x,a.y)));
    } else if index == 2. {
        icon = (1.-smoothstep(0.45,0.45+aa,abs(q.y))) * (1.-smoothstep(5.,5.+aa,abs(q.x)));
    } else if index == 3. {
        if overlay.state.z > 0.5 {
            let front = q-vec2(-1.5,1.5);
            let back = q-vec2(1.5,-1.5);
            let back_visible = select(1.,0.,max(abs(front.x),abs(front.y)) < 4.5);
            icon = max(square_outline(front,4.,aa),square_outline(back,4.,aa)*back_visible);
        } else { icon = square_outline(q,5.,aa); }
    } else {
        let diagonal = min(abs(q.x-q.y),abs(q.x+q.y)) * 0.7071;
        icon = (1.-smoothstep(0.55,0.55+aa,diagonal)) * (1.-smoothstep(5.,5.+aa,max(abs(q.x),abs(q.y))));
    }
    color = mix(color,vec3(0.88),icon);
    let outline = (1.-smoothstep(0.,0.8,-distance))*0.13;
    color = mix(color,vec3(0.4),outline);
    return vec4(color,coverage*overlay.screen.w*0.97);
}
