// Declare @svg.sdf as a real pass input. The native contour renderer overlays
// this clear pass, as Helix overlays its native renderer. textureLoad deliberately
// uses the full-precision Rgba32Float SDF without requiring float32 filtering.
@fragment
fn fs_main(@builtin(position) position: vec4f) -> @location(0) vec4f {
    let field = textureLoad(input0_tex, vec2i(0), 0);
    return vec4f(0.0, 0.0, 0.0, min(abs(field.x), 1.0)*0.0);
}
