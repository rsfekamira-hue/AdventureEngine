struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) settings: vec4<f32>,
    @location(3) animation: vec2<f32>,
};
@group(0) @binding(0) var atlas: texture_2d<f32>;
@group(0) @binding(1) var atlas_sampler: sampler;
@vertex fn vs_main(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>, @location(3) settings: vec4<f32>, @location(4) animation: vec2<f32>) -> VertexOut {
    return VertexOut(vec4<f32>(position, 0., 1.), uv, color, settings, animation);
}
fn line(d: f32, width: f32) -> f32 { return 1.-smoothstep(width, width+1., abs(d)); }
// Signed distance to a rectangle with clipped diagonal corners, in display points.
fn panel_distance(p: vec2<f32>, size: vec2<f32>, cut: f32) -> f32 {
    let q = abs(p-size*0.5)-size*0.5;
    return max(max(q.x,q.y), (q.x+q.y+cut)*0.70710678);
}
fn shade(v: VertexOut) -> vec4<f32> {
    var color = v.color;
    if v.settings.x < 0.5 {
        color.a *= textureSample(atlas, atlas_sampler, v.uv).r;
    } else if v.settings.x < 1.5 || v.settings.x > 2.5 {
        let uv = v.uv;
        let size = max(v.settings.zw, vec2<f32>(1.));
        let p = uv*size;
        let time = v.animation.x;
        let inset = vec2<f32>(5.,5.);
        let d = panel_distance(p-inset, size-inset*2., min(19.,size.y*0.15));
        let mask = 1.-smoothstep(-0.5,0.5,d);
        let back_d = panel_distance(p-vec2<f32>(9.,10.),size-inset*2.,min(19.,size.y*0.15));
        let back = (1.-smoothstep(0.,1.,back_d))*(1.-mask);
        let edge = line(d+1.5,0.65)*mask;
        let rim = line(d+4.,0.5)*0.25*mask;
        let glow = exp(-abs(d+1.)*0.26)*0.22*mask;
        let light = 0.35+0.65*(1.-uv.y);
        // A translucent, subtly lit face with dark extruded edges below it.
        var rgb = vec3<f32>(0.018,0.045,0.08)*(0.8+0.4*(1.-uv.y));
        var alpha = (0.50+0.05*uv.y)*mask+back*0.30;
        var ink = edge*light+rim+glow;
        let grid_uv = (p+vec2<f32>(time*4.,-time*7.))/32.;
        let grid = max(line(fract(grid_uv.x)*32.,0.3),line(fract(grid_uv.y)*32.,0.3));
        let quiet = smoothstep(0.38,0.85,uv.x);
        ink += grid*0.05*(0.3+quiet)*mask;
        // Header rail, inset baseline, and segmented lower telemetry strip.
        ink += line(p.y-size.y*0.145,0.5)*select(0.,0.25, uv.x>0.045 && uv.x<0.95)*mask;
        ink += line(p.x-12.,1.)*select(0.,0.8, uv.y>0.20 && uv.y<0.72)*mask;
        let ticks = select(0.,1.,fract(p.x/9.)<0.2);
        ink += line(p.y-size.y+15.,2.2)*ticks*0.35*mask;
        // Packets travel along horizontal data lanes behind the contents.
        let lanes = floor(p.y/29.);
        let lane = line(fract(p.y/29.)*29.-15.,0.6);
        let packet = pow(max(0.,1.-fract(uv.x*1.6-time*0.12+lanes*0.137)),20.);
        ink += lane*(0.06+packet*0.55)*(0.2+quiet)*mask;
        // A pair of counter-rotating, broken diagnostic rings on the right.
        if size.y > 110. {
            let ring_p = p-vec2<f32>(size.x*0.81,size.y*0.53);
            let radius = length(ring_p);
            let angle = atan2(ring_p.y,ring_p.x);
            let ring_r = min(size.y*0.25,size.x*0.15);
            let segments = select(0.16,0.75, sin(angle*5.+time*0.6)>0.15);
            ink += line(radius-ring_r,0.7)*segments*0.5*mask;
            ink += line(radius-ring_r*0.82,1.5)*select(0.05,0.38,sin(angle*3.-time*0.5)>0.35)*mask;
            ink += line(radius-ring_r*1.13,0.4)*select(0.,0.3,fract(angle*10.)>0.7)*mask;
            ink += (line(ring_p.x,0.4)+line(ring_p.y,0.4))*select(0.,0.12,radius<ring_r*0.65)*mask;
            let circuit = line(abs(ring_p.y)-ring_r*0.5,0.5)*select(0.,0.24,abs(ring_p.x)>ring_r && abs(ring_p.x)<ring_r*1.7);
            ink += circuit*mask;
        }
        let scan = exp(-abs(uv.y-fract(time*0.10))*size.y*0.12)*0.09;
        ink += scan*mask;
        if v.settings.x > 2.5 {
            let row = floor(uv.y*18.);
            let shift = select(0.,sin(v.settings.y*7.+row*0.7)*0.06,v.settings.x>3.5);
            let cell = fract(vec2<f32>(uv.x+shift,uv.y)*vec2<f32>(8.,18.));
            let code = fract(sin(floor((uv.x+shift)*8.)*13.+row*17.+floor(v.settings.y*42.))*43758.5453);
            ink += select(0.,0.22,cell.x<0.65 && cell.y<0.25 && code>0.25)*mask;
        }
        rgb = mix(rgb,v.color.rgb,clamp(ink,0.,1.));
        // Thin near-white highlights and a dim lower edge give the frame thickness.
        rgb += vec3<f32>(0.2,0.28,0.3)*edge*light;
        alpha = clamp(alpha+ink*0.45,0.,0.92)*v.color.a;
        color = vec4<f32>(rgb,alpha);
    } else {
        // Segmented progress track, using its own local UV.
        color.a *= select(0.6,1.,fract(v.uv.x*40.)>0.09);
    }
    return color;
}
@fragment fn fs_gamma(v: VertexOut) -> @location(0) vec4<f32> {
    let c = shade(v); return vec4<f32>(c.rgb*c.a,c.a);
}
@fragment fn fs_linear(v: VertexOut) -> @location(0) vec4<f32> {
    let c = shade(v);
    let rgb = select(c.rgb/12.92,pow((c.rgb+0.055)/1.055,vec3<f32>(2.4)),c.rgb>vec3<f32>(0.04045));
    return vec4<f32>(rgb*c.a,c.a);
}
