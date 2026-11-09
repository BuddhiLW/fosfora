// Cached JFA+1 distance transform. Run only on SVG/quality/resolution change.
struct Config { size: vec2u, step: u32, inside: u32 }
@group(0) @binding(0) var<uniform> cfg: Config;
@group(0) @binding(1) var ids: texture_2d<u32>;
@group(0) @binding(2) var seeds: texture_2d<f32>;
@group(0) @binding(3) var next: texture_storage_2d<rg32float, write>;
@group(0) @binding(4) var field: texture_storage_2d<rgba32float, write>;
@group(0) @binding(5) var<storage, read_write> radii: array<atomic<u32>>;
@group(0) @binding(6) var<storage, read_write> interior: array<f32>;

fn id_at(p: vec2i) -> u32 {
    if any(p < vec2i(0)) || any(p >= vec2i(cfg.size)) { return 0u; }
    return textureLoad(ids, p, 0).x;
}
fn valid(p: vec2i) -> bool { return all(p >= vec2i(0)) && all(p < vec2i(cfg.size)); }
@compute @workgroup_size(8, 8)
fn seed(@builtin(global_invocation_id) g: vec3u) {
    if any(g.xy >= cfg.size) { return; }
    let p = vec2i(g.xy);
    let id = id_at(p);
    let edge = id != 0u && (id_at(p+vec2i(1,0)) != id || id_at(p-vec2i(1,0)) != id ||
        id_at(p+vec2i(0,1)) != id || id_at(p-vec2i(0,1)) != id);
    textureStore(next, p, vec4f(select(vec2f(-1.0), vec2f(p), edge), 0.0, 0.0));
}
@compute @workgroup_size(8, 8)
fn jump(@builtin(global_invocation_id) g: vec3u) {
    if any(g.xy >= cfg.size) { return; }
    let p = vec2i(g.xy);
    let id = id_at(p);
    var best = vec2f(-1.0);
    var best_d = 1e30;
    for(var y = -1; y <= 1; y++) { for(var x = -1; x <= 1; x++) {
        let q = p + vec2i(x,y)*i32(cfg.step);
        if !valid(q) { continue; }
        let s = textureLoad(seeds, q, 0).xy;
        if s.x < 0.0 { continue; }
        if cfg.inside != 0u && id_at(vec2i(s)) != id { continue; }
        let delta = s - vec2f(p);
        let d = dot(delta, delta);
        if d < best_d { best = s; best_d = d; }
    }}
    textureStore(next, p, vec4f(best, 0.0, 0.0));
}
@compute @workgroup_size(8, 8)
fn reduce(@builtin(global_invocation_id) g: vec3u) {
    if any(g.xy >= cfg.size) { return; }
    let p = vec2i(g.xy);
    let id = id_at(p);
    let s = textureLoad(seeds, p, 0).xy;
    var d = 0.0;
    if id != 0u && s.x >= 0.0 { d = length(s-vec2f(p)) + 0.5; }
    interior[g.y*cfg.size.x + g.x] = d;
    if id != 0u { atomicMax(&radii[id], bitcast<u32>(d)); }
}
@compute @workgroup_size(8, 8)
fn finish(@builtin(global_invocation_id) g: vec3u) {
    if any(g.xy >= cfg.size) { return; }
    let p = vec2i(g.xy);
    var id = id_at(p);
    let s = textureLoad(seeds, p, 0).xy;
    var d = f32(max(cfg.size.x, cfg.size.y));
    if id != 0u {
        d = -interior[g.y*cfg.size.x+g.x];
    } else if s.x >= 0.0 {
        d = max(0.5, length(s-vec2f(p))-0.5);
        id = id_at(vec2i(s));
    }
    let radius = max(0.5, bitcast<f32>(atomicLoad(&radii[id])));
    textureStore(field, p, vec4f(d, f32(id), d/radius, radius));
}
