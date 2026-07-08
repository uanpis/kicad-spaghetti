struct ScreenInfo {
    size: vec2<u32>,
    pan: vec2<f32>,
    zoom: f32,
    aspect_ratio: f32,
}
@group(0) @binding(0) var<uniform> screen_info: ScreenInfo;

struct VertIn {
    @location(0) uv: vec2<f32>,
    @location(1) p0: vec2<f32>,
    @location(2) p1: vec2<f32>,
    @location(3) p2: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) radius: f32,
}

struct VertOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) p0: vec2<f32>,
    @location(2) p1: vec2<f32>,
    @location(3) p2: vec2<f32>,
    @location(4) n01: vec2<f32>,
    @location(5) n12: vec2<f32>,
    @location(6) n20: vec2<f32>,
    @location(7) color: vec4<f32>,
    @location(8) radius: f32,
    @location(9) scale: f32,
}

fn to_screen_space(world: vec2<f32>) -> vec4<f32> {
    let v = (world + screen_info.pan) * screen_info.zoom;
    return vec4<f32>(v.x / screen_info.aspect_ratio, -v.y, 0.0, 1.0);
}

fn edge_normal(edge: vec2<f32>) -> vec2<f32> {
    return normalize(vec2(-edge.y, edge.x));
}

fn corner_normal(edge_normal_0: vec2<f32>, edge_normal_1: vec2<f32>) -> vec2<f32> {
    let sum = edge_normal_0 + edge_normal_1;
    let len = length(sum);
    return normalize(sum) / len;
}

    @vertex
fn vs_main(in: VertIn) -> VertOut {
    let pixel_size = 2.0 / (screen_info.zoom * f32(screen_info.size.y));

    let fac0 = 1 - in.uv.x - in.uv.y;
    let fac1 = in.uv.x;
    let fac2 = in.uv.y;

    let n01 = edge_normal(in.p1 - in.p0);
    let n12 = edge_normal(in.p2 - in.p1);
    let n20 = edge_normal(in.p0 - in.p2);

    let n0 = corner_normal(n20, n01);
    let n1 = corner_normal(n01, n12);
    let n2 = corner_normal(n12, n20);

    let p0_expanded = in.p0 + 2.0 * (pixel_size + in.radius) * n0;
    let p1_expanded = in.p1 + 2.0 * (pixel_size + in.radius) * n1;
    let p2_expanded = in.p2 + 2.0 * (pixel_size + in.radius) * n2;

    let world = fac0 * p0_expanded + fac1 * p1_expanded + fac2 * p2_expanded;

    var out: VertOut;
    out.pos = to_screen_space(world);
    out.world = world;
    out.p0 = in.p0;
    out.p1 = in.p1;
    out.p2 = in.p2;
    out.n01 = n01;
    out.n12 = n12;
    out.n20 = n20;
    out.color = in.color;
    out.radius = in.radius;
    out.scale = f32(screen_info.size.y) * screen_info.zoom;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let hollow = false;

    let edge01 = in.p1 - in.p0;
    let edge12 = in.p2 - in.p1;
    let edge20 = in.p0 - in.p2;
    let fac01 = clamp(dot(in.world - in.p0, normalize(edge01) / length(edge01)), 0.0, 1.0);
    let fac12 = clamp(dot(in.world - in.p1, normalize(edge12) / length(edge12)), 0.0, 1.0);
    let fac20 = clamp(dot(in.world - in.p2, normalize(edge20) / length(edge20)), 0.0, 1.0);
    var d01 = distance(in.world, mix(in.p0, in.p1, fac01));
    var d12 = distance(in.world, mix(in.p1, in.p2, fac12));
    var d20 = distance(in.world, mix(in.p2, in.p0, fac20));
    if !hollow {
        d01 *= sign(dot(in.world - in.p0, in.n01));
        d12 *= sign(dot(in.world - in.p1, in.n12));
        d20 *= sign(dot(in.world - in.p2, in.n20));
    }

    let d = (max(d01, max(d12, d20)) - in.radius) * in.scale;

    // feathering only outside actual shape, to avoid transparency between
    // adjacent triangles
    if d > 2.0 { discard; }
    let alpha = 1.0 - smoothstep(0.0, 2.0, d);
    return vec4<f32>(in.color.rgb * in.color.rgb, in.color.a * in.color.a * alpha);
}
