struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
};

@vertex
fn vs_main(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>,
           @location(2) tint: vec4<f32>) -> VertexOutput {
    return VertexOutput(vec4<f32>(position, 0.0, 1.0), uv, tint);
}

@group(0) @binding(0) var image: texture_2d<f32>;

fn premultiplied_at(p: vec2<i32>, size: vec2<i32>) -> vec4<f32> {
    // The sRGB texture supplies linear RGB. Premultiply BEFORE interpolation:
    // transparent texels must not darken or color an opaque edge.
    let value = textureLoad(image, clamp(p, vec2<i32>(0), size - vec2<i32>(1)), 0);
    return vec4<f32>(value.rgb * value.a, value.a);
}

fn sample_image(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(image));
    let p = uv * vec2<f32>(size) - vec2<f32>(0.5);
    let base = vec2<i32>(floor(p));
    let f = fract(p);
    return mix(
        mix(premultiplied_at(base, size), premultiplied_at(base + vec2<i32>(1, 0), size), f.x),
        mix(premultiplied_at(base + vec2<i32>(0, 1), size), premultiplied_at(base + vec2<i32>(1, 1), size), f.x),
        f.y);
}

fn gamma_from_linear(value: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(value, vec3<f32>(1.0 / 2.4)) - 0.055,
                  12.92 * value, value <= vec3<f32>(0.0031308));
}

fn linear_from_gamma(value: vec3<f32>) -> vec3<f32> {
    return select(pow((value + 0.055) / 1.055, vec3<f32>(2.4)),
                  value / 12.92, value <= vec3<f32>(0.04045));
}

fn composite_color(input: VertexOutput) -> vec4<f32> {
    let value = sample_image(input.uv);
    // Match the UI's gamma-space tint and compositing convention without
    // converting or retaining a second copy of the entire source image.
    return vec4<f32>(gamma_from_linear(value.rgb), value.a) * input.tint;
}

@fragment
fn fs_gamma(input: VertexOutput) -> @location(0) vec4<f32> {
    return composite_color(input);
}

@fragment
fn fs_linear(input: VertexOutput) -> @location(0) vec4<f32> {
    let value = composite_color(input);
    return vec4<f32>(linear_from_gamma(value.rgb), value.a);
}
