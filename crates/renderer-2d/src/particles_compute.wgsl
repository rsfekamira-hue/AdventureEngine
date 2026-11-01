struct Params {
    spawn: vec4f, motion: vec4f, acceleration_size: vec4f, destination: vec4f,
    color: vec4f, settings: vec4f, ids: vec4u,
    rect: vec4f, screen: vec4f, clock: vec4f, tint: vec4f,
};
struct Particle { geometry: vec4f, color: vec4f, extra: vec4f };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;
fn hash(input: u32) -> u32 {
    var x = input;
    x = (x ^ (x >> 16u)) * 0x7feb352du;
    x = (x ^ (x >> 15u)) * 0x846ca68bu;
    return x ^ (x >> 16u);
}
fn random(seed: u32, lane: u32) -> f32 {return f32(hash(seed + lane * 0x9e3779b9u)) / 4294967295.0;}
@compute @workgroup_size(64)
fn simulate(@builtin(global_invocation_id) id: vec3u) {
    let index = id.x;
    if index >= p.ids.y * (p.ids.z + 1u) {return;}
    let particle = index / (p.ids.z + 1u);
    let trail = p.ids.z - index % (p.ids.z + 1u);
    let base = p.ids.x + particle * 7919u;
    let clock = max(p.clock.x, 0.0) + random(base, 0u) * p.settings.x;
    let seed = base + u32(floor(clock / p.settings.x)) * 104729u;
    let age = clock % p.settings.x - f32(trail) * p.clock.z;
    var out: Particle;
    if age < 0.0 {particles[index] = out; return;}
    let t = age / p.settings.x;
    let seconds = age * 0.001;
    var position = p.spawn.xy + p.spawn.zw * vec2f(random(seed,1u),random(seed,2u))
        + (p.motion.xy + p.motion.zw * (2.0 * vec2f(random(seed,3u),random(seed,4u))-1.0)) * seconds
        + p.acceleration_size.xy * seconds * seconds * 0.5;
    if p.destination.z >= 0.0 {
        let goal = p.destination.xy + p.destination.zw * vec2f(random(seed,5u),random(seed,6u));
        position = mix(position,goal,t*t);
    }
    let noise = random(seed,u32(age / 90.0)+20u);
    let glitch = select(0.0,p.settings.z*noise,noise>0.72);
    position.x += glitch * (noise - 0.85) * 0.3;
    let glyph = hash(seed + u32(floor(age / p.settings.w))) % p.ids.w;
    out.geometry = vec4f(position,mix(p.acceleration_size.z,p.acceleration_size.w,t),f32(glyph));
    out.color = p.color * p.tint;
    out.color.a *= min(t*12.0,1.0)*min((1.0-t)*5.0,1.0)*pow(0.68,f32(trail))*p.clock.y;
    out.extra = vec4f(select(0.0,p.settings.y,trail==0u),glitch,p.clock.w,0.0);
    particles[index] = out;
}
