struct Globals {
    zoom:         vec2<f32>,
    pan:          vec2<f32>,
    aspect_ratio: f32,
    _pad:         f32,
}
@group(0) @binding(0) var<uniform> globals: Globals;

struct VertIn {
    @location(0) quad_pos: vec2<f32>, // [0,1]^2 unit quad
    @location(1) center:   vec2<f32>,
    @location(2) radius:   f32,
    @location(3) color:    vec4<f32>,
}

struct VertOut {
    @builtin(position) clip_pos:  vec4<f32>,
    @location(0)       local_pos: vec2<f32>, // in [-1,1]^2, normalised by radius
    @location(1)       color:     vec4<f32>,
    @location(2)       radius_px: f32,       // world-space radius after zoom
}

@vertex
fn vs_main(in: VertIn) -> VertOut {
    // Map [0,1]^2 quad to [-r, r]^2 bounding box around centre
    let local  = (in.quad_pos * 2.0 - vec2<f32>(1.0)) * in.radius;
    let world  = in.center + local;

    // Same transform the edge shader uses
    let zoomed = (world + globals.pan) * globals.zoom;

    var out: VertOut;
    out.clip_pos  = vec4<f32>(zoomed.x / globals.aspect_ratio, zoomed.y, 0.0, 1.0);
    out.local_pos = local / in.radius;           // normalised: circle edge at ‖local_pos‖ = 1
    out.color     = in.color;
    out.radius_px = in.radius * globals.zoom.x;  // for feather width
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let dist = length(in.local_pos);

	/*
    // Feather ~1.5 pixels wide regardless of zoom
    let feather = 1.5 / max(in.radius_px, 1.0);
    let alpha   = smoothstep(1.0, 1.0 - feather, dist);
    if alpha <= 0.0 { discard; }
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
	*/

	if dist > in.radius_px {
		discard;
	}
    return vec4<f32>(in.color.rgba);

}
