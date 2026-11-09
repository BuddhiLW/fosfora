struct Draw {
    params: array<vec4f, 4>,
    geometry: vec4f, // field width/height, shape count, background
    transform: vec4f, // offset, rotation, output aspect
    key: vec4f, // width, phase, softness, repeat
}
struct Shape { color: vec4f, geometry: vec4f, bounds: vec4f }
@group(0) @binding(0) var<uniform> u: Draw;
@group(0) @binding(1) var sdf: texture_2d<f32>;
@group(0) @binding(2) var<storage, read> shapes: array<Shape>;
fn p(i: u32) -> f32 { return u.params[i/4u][i%4u]; }
fn hsv(h: f32) -> vec3f { return clamp(abs(fract(h+vec3f(0.0, 2.0/3.0, 1.0/3.0))*6.0-3.0)-1.0, vec3f(0.0), vec3f(1.0)); }
fn hash(n: f32) -> f32 { return fract(sin(n*127.1+31.7)*43758.5453); }
@fragment
fn fs_main(@builtin(position) position: vec4f, @location(0) uv: vec2f) -> @location(0) vec4f {
    // Vertex UV and tiny-skia both have their origin at the top-left.
    var q = uv-0.5-u.transform.xy;
    q.x *= u.transform.w;
    let c = cos(u.transform.z); let s = sin(u.transform.z);
    q = mat2x2f(c, -s, s, c)*q;
    q.x /= u.transform.w;
    q = q/max(0.1, p(12u))+0.5;
    var bg = vec4f(0.0);
    if u.geometry.w > 0.5 { bg = vec4f(vec3f(select(0.0, 1.0, u.geometry.w > 1.5)), 1.0); }
    if any(q < vec2f(0.0)) || any(q >= vec2f(1.0)) { return bg; }
    let f = textureLoad(sdf, vec2i(q*u.geometry.xy), 0);
    let id = u32(f.y);
    if id == 0u || id >= arrayLength(&shapes) { return bg; }
    let shape = shapes[id];
    var order = length((shape.geometry.xy-0.5)*vec2f(u.transform.w, 1.0));
    switch i32(p(7u)) {
        case 1: { order = shape.geometry.y; }
        case 2: { order = hash(shape.geometry.z); }
        case 3: { order = hash(f32(id)); }
        default: {}
    }
    // Slots 13/14 are CPU integrals of speed/hue_speed, never time*rate.
    var motion = fract(p(13u)+p(4u));
    if p(5u) > 0.5 { motion = 1.0-abs(fract((p(13u)+p(4u))*0.5)*2.0-1.0); }
    let amount = max(1.0, p(0u));
    let distance = -f.x/max(0.5, f.w);
    let ring_phase = distance*amount-motion+order;
    let band = abs(fract(ring_phase+0.5)-0.5);
    let aa = max(0.01, fwidth(ring_phase));
    var alpha = 1.0-smoothstep(p(1u)*0.5-aa, p(1u)*0.5+aa, band);
    // Shrink=1 confines rings to the silhouette; 0 permits outer offsets.
    let limit = (1.0-clamp(p(2u), 0.0, 2.0))*max(u.geometry.x,u.geometry.y)*0.12;
    alpha *= 1.0-smoothstep(limit-1.0,limit+1.0,f.x);
    if p(2u) > 1.0 { alpha *= smoothstep(p(2u)-1.0, p(2u)-0.98, distance); }
    alpha *= exp(-abs(distance)*max(0.0,p(8u)));
    let key = fract(distance*max(0.01,u.key.w)+u.key.y);
    if u.key.x < 0.999 { alpha *= 1.0-smoothstep(u.key.x, u.key.x+max(0.001,u.key.z), key); }
    var color = vec3f(1.0);
    switch i32(p(6u)) {
        case 0: { color = vec3f(0.3+0.7*fract(distance*amount/4.0+motion)); }
        case 1: { color = hsv(order+distance*0.7+p(14u)+p(11u)); }
        case 3: { color = shape.color.rgb; }
        default: {}
    }
    alpha = clamp(alpha*shape.color.a,0.0,1.0);
    let rgb = color*max(0.0,p(9u))*alpha;
    return vec4f(rgb+bg.rgb*(1.0-alpha), alpha+bg.a*(1.0-alpha));
}
