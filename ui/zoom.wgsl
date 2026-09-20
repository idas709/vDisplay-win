struct Panel { screen: vec4<f32>, rect: vec4<f32> }
@group(0) @binding(0) var panel_texture: texture_2d<f32>;
@group(0) @binding(1) var panel_sampler: sampler;
@group(0) @binding(2) var<uniform> panel: Panel;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> Vertex {
    var corners = array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
    let uv = corners[i]; let p = panel.rect.xy + uv * panel.rect.zw;
    var out: Vertex;
    out.position = vec4(p.x/panel.screen.x*2.-1.,1.-p.y/panel.screen.y*2.,0.,1.);
    out.uv = uv; return out;
}
@fragment fn fs_main(input: Vertex) -> @location(0) vec4<f32> {
    let color = textureSample(panel_texture,panel_sampler,input.uv);
    return vec4(color.rgb,color.a*panel.screen.z*0.98);
}
