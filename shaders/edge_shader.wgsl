struct Globals {
    zoom:         vec2<f32>,
    pan:          vec2<f32>,
    aspect_ratio: f32,
    _pad:         f32,
}
@group(0) @binding(0) var<uniform> globals: Globals;

struct VertOut {
    @builtin(position) clip_pos:  vec4<f32>,
    @location(0)       world_pos: vec2<f32>,
    @location(1)       p0:        vec2<f32>,
    @location(2)       p1:        vec2<f32>,
    @location(3)       radius:    f32,
    @location(4)       color:     vec4<f32>,
}

fn to_clip(world: vec2<f32>) -> vec4<f32> {
    // Match whatever transform your existing lyon_shader.wgsl uses.
    let ndc = (world + globals.pan) * globals.zoom;
    return vec4(ndc.x / globals.aspect_ratio, ndc.y, 0.0, 1.0);
}

@vertex
fn vs_main(
    @location(0) uv:     vec2<f32>,   // quad corner in [0,1]²
    @location(1) p0:     vec2<f32>,
    @location(2) p1:     vec2<f32>,
    @location(3) radius: f32,
    @location(4) color:  vec4<f32>,
) -> VertOut {
    let aabb_min = min(p0, p1) - radius;
    let aabb_max = max(p0, p1) + radius;
    let world_pos = mix(aabb_min, aabb_max, uv);

    var out: VertOut;
    out.clip_pos  = to_clip(world_pos);
    out.world_pos = world_pos;
    out.p0        = p0;
    out.p1        = p1;
    out.radius    = radius;
    out.color     = color;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let pa = in.world_pos - in.p0;
    let ba = in.p1 - in.p0;
    let h  = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    let d  = length(pa - ba * h) - in.radius;

    //if d > 1.0 { discard; }
    //let alpha = 1.0 - smoothstep(-1.0, 0.0, d);
    if d > 0.0 { discard; }
    let alpha = 1.0;
    return vec4(in.color.rgb, in.color.a * alpha);
}
