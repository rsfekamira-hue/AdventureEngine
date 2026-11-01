struct Params {
    spawn: vec4f, motion: vec4f, acceleration_size: vec4f, destination: vec4f,
    color: vec4f, settings: vec4f, ids: vec4u,
    rect: vec4f, screen: vec4f, clock: vec4f, tint: vec4f,
};
struct Glyph {uv: vec4f, metrics: vec4f};
@group(0) @binding(0) var<uniform> p: Params;
struct GlyphTable {entries:array<Glyph,128>};
@group(0) @binding(1) var<uniform> glyphs: GlyphTable;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(0) @binding(3) var atlas_sampler: sampler;
struct Out {
    @builtin(position) position: vec4f,
    @location(0) local: vec2f,
    @location(1) color: vec4f,
    @location(2) @interpolate(flat) uv: vec4f,
    @location(3) @interpolate(flat) extra: vec4f,
};
@vertex
fn vertex(@builtin(vertex_index) index:u32,@location(0) geometry:vec4f,@location(1) color:vec4f,@location(2) extra:vec4f)->Out {
    let corners=array<vec2f,6>(vec2f(-1,-1),vec2f(1,-1),vec2f(-1,1),vec2f(-1,1),vec2f(1,-1),vec2f(1,1));
    let glyph=glyphs.entries[u32(geometry.w)];
    let local=corners[index]*1.3;
    // Atlas cells include baseline/padding; preserve the existing visible text size.
    let size=p.rect.w*geometry.z*select(1.0,2.0,extra.z>0.5);
    let point=p.rect.xy+p.rect.zw*geometry.xy+local*vec2f(size*glyph.metrics.x,size)*0.5;
    var out:Out;
    out.position=vec4f(2.0*(point.x-p.screen.x)/p.screen.z-1.0,1.0-2.0*(point.y-p.screen.y)/p.screen.w,0,1);
    out.local=local;
    out.color=color;
    out.uv=glyph.uv;
    out.extra=extra;
    return out;
}
fn coverage(uv:vec2f,rect:vec4f)->f32 {
    let inside=all(uv>=vec2f(0)) && all(uv<=vec2f(1));
    return textureSample(atlas,atlas_sampler,rect.xy+clamp(uv,vec2f(0),vec2f(1))*rect.zw).r*select(0.0,1.0,inside);
}
fn shade(in:Out)->vec4f {
    let uv=in.local*0.5+0.5;
    let edge=max(abs(in.local.x),abs(in.local.y));
    var base=1.0-smoothstep(0.96,1.0,edge);
    var halo=(1.0-smoothstep(0.85,1.3,edge))*in.extra.x*0.35;
    if in.extra.z>0.5 {
        base=coverage(uv,in.uv);
        let offset=vec2f(0.025,0.0);
        halo=max(max(coverage(uv+offset,in.uv),coverage(uv-offset,in.uv)),max(coverage(uv+offset.yx,in.uv),coverage(uv-offset.yx,in.uv)))*in.extra.x*0.35;
    }
    let alpha=base*in.color.a;
    // One quad: a premultiplied body plus additive light, no CPU duplicate draws.
    return vec4f(in.color.rgb*(alpha+halo*in.color.a),alpha);
}
@fragment fn gamma(in:Out)->@location(0) vec4f {return shade(in);}
@fragment fn linear(in:Out)->@location(0) vec4f {
    let c=shade(in);
    return vec4f(select(c.rgb/12.92,pow((c.rgb+0.055)/1.055,vec3f(2.4)),c.rgb>vec3f(0.04045)),c.a);
}
