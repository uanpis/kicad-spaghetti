struct Globals {
    zoom: vec2<f32>,
    pan: vec2<f32>,
    aspect_ratio: f32,
    _pad: f32,
}
@group(0) @binding(0) var<uniform> globals: Globals;

struct VertexIn {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    // pan then zoom, correct for aspect ratio
    let p = (in.position + globals.pan) * globals.zoom;
    var out: VertexOut;
    out.clip_position = vec4<f32>(p.x / globals.aspect_ratio, p.y, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
