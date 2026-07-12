const LAYER_MULTIPLIER = 0.001f;

struct Globals {
    size: vec2<u32>,
    pan: vec2<f32>,
    zoom: f32,
    aspect_ratio: f32,
}
@group(0) @binding(0) var<uniform> globals: Globals;

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       world_pos: vec2<f32>,
    @location(1)       p0: vec2<f32>,
    @location(2)       p1: vec2<f32>,
    @location(3)       radius: f32,
    @location(4)       scale: f32,
    @location(5)       color: vec4<f32>,
}

fn to_screen_space(world: vec2<f32>, layer: u32) -> vec4<f32> {
    let v = (world + globals.pan) * globals.zoom;
    return vec4<f32>(
        v.x / globals.aspect_ratio,
        -v.y,
        f32(layer) * LAYER_MULTIPLIER,
        1.0
    );
}

@vertex
fn vs_main(
    @location(0) uv: vec2<f32>,
    @location(1) p0: vec2<f32>,
    @location(2) p1: vec2<f32>,
    @location(3) radius: f32,
    @location(4) color: vec4<f32>,
    @location(5) layer: u32,
) -> VertOut {

    let pixel_size = 2.0 / (globals.zoom * f32(globals.size.y));
    let pixel_offset = (uv - 0.5) * pixel_size;

    let aabb_min = min(p0, p1) - radius;
    let aabb_max = max(p0, p1) + radius;
    let world_pos = mix(aabb_min, aabb_max, uv) + pixel_offset;

    var out: VertOut;
    out.clip_pos = to_screen_space(world_pos, layer);
    out.world_pos = world_pos;
    out.p0 = p0;
    out.p1 = p1;
    out.radius = radius; // mm
    out.scale = f32(globals.size.y) * globals.zoom; // px / mm
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let pa = in.world_pos - in.p0;
    let ba = in.p1 - in.p0;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    let d = (length(pa - ba * h) - in.radius) * in.scale;

    if d > 1.0 { discard; }
    let alpha = 1.0 - smoothstep(-1.0, 1.0, d);
    return vec4(in.color.rgb * in.color.rgb, in.color.a * in.color.a * alpha);
}
