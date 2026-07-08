struct ScreenInfo {
    size: vec2<u32>,
    pan: vec2<f32>,
    zoom: f32,
    aspect_ratio: f32,
}
@group(0) @binding(0) var<uniform> screen_info: ScreenInfo;

struct VertIn {
    @location(0) uv: vec2<f32>,
    @location(1) center: vec2<f32>,
    @location(2) radius: f32,
    @location(3) color: vec4<f32>,
}

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       local_pos: vec2<f32>,
    @location(1)       color: vec4<f32>,
    @location(2)       radius: f32,
    @location(3)       scale: f32,
}

fn to_screen_space(world: vec2<f32>) -> vec4<f32> {
    let v = (world + screen_info.pan) * screen_info.zoom;
    return vec4(v.x / screen_info.aspect_ratio, -v.y, 0.0, 1.0);
}

@vertex
fn vs_main(in: VertIn) -> VertOut {
    let pixel_size = 1.0 / (screen_info.zoom * f32(screen_info.size.y));
    let pixel_offset = (in.uv - 0.5) * pixel_size;

    let local = (in.uv * 2.0 - vec2<f32>(1.0)) * in.radius + pixel_offset;
    let world = in.center + local;

    var out: VertOut;
    out.clip_pos = to_screen_space(world);
    out.local_pos = local;
    out.color = in.color;
    out.radius = in.radius;
    out.scale = f32(screen_info.size.y) * screen_info.zoom;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let dist = length(in.local_pos);
    let d = (dist - in.radius) * in.scale;

    if d > 1.0 { discard; }
    let alpha = 1.0 - smoothstep(-1.0, 1.0, d);
    return vec4<f32>(in.color.rgb * in.color.rgb, in.color.a * in.color.a * alpha);
}
