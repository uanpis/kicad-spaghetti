struct ScreenInfo {
    size: vec2<u32>,
    pan: vec2<f32>,
    zoom: f32,
    aspect_ratio: f32,
}
@group(0) @binding(0) var<uniform> screen_info: ScreenInfo;

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

fn to_screen_space(world: vec2<f32>) -> vec4<f32> {
    let v = (world + screen_info.pan) * screen_info.zoom;
    return vec4(v.x / screen_info.aspect_ratio, -v.y, 0.0, 1.0);
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = to_screen_space(in.position);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
