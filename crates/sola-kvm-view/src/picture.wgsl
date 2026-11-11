// One NV12 picture, BT.709 limited range, on a quad; and the other
// machine's pointer, premultiplied RGBA, on a quad over it.

struct Uniforms {
    // The quad in clip space: left, top, right, bottom.
    rect: vec4<f32>,
    // Nonzero when the target encodes to sRGB itself.
    srgb_target: u32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var luma: texture_2d<f32>;
@group(0) @binding(2) var chroma: texture_2d<f32>;
@group(0) @binding(3) var picture_sampler: sampler;
// The pointer's pipeline binds this and the uniforms, not the planes.
@group(0) @binding(4) var pointer_image: texture_2d<f32>;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) index: u32) -> VertexOut {
    let corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let c = corners[index];
    var out: VertexOut;
    out.position = vec4(mix(u.rect.x, u.rect.z, c.x), mix(u.rect.y, u.rect.w, c.y), 0.0, 1.0);
    out.uv = c;
    return out;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + vec3(0.055)) / 1.055, vec3(2.4));
    return select(high, low, c <= vec3(0.04045));
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4<f32> {
    let y = (textureSample(luma, picture_sampler, in.uv).r - 16.0 / 255.0) * (255.0 / 219.0);
    let c = (textureSample(chroma, picture_sampler, in.uv).rg - vec2(128.0 / 255.0)) * (255.0 / 224.0);
    var rgb = clamp(
        vec3(y + 1.5748 * c.g, y - 0.1873 * c.r - 0.4681 * c.g, y + 1.8556 * c.r),
        vec3(0.0),
        vec3(1.0),
    );
    if u.srgb_target != 0u {
        rgb = to_linear(rgb);
    }
    return vec4(rgb, 1.0);
}

@fragment
fn fs_pointer(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(pointer_image, picture_sampler, in.uv);
    if c.a <= 0.0 {
        return vec4(0.0);
    }
    var rgb = c.rgb / c.a;
    if u.srgb_target != 0u {
        rgb = to_linear(rgb);
    }
    return vec4(rgb * c.a, c.a);
}
