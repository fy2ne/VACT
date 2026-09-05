// VACT V4/V5 — Pass 1: Bilateral Filter (Universal RGBA8)
//
// Luminance smoothing with edge preservation.
// Input:  input_tex — Rgba8Unorm desktop frame (swizzled [B, G, R, A])
// Output: luma_tex  — Rgba8Unorm smoothed luminance (R=G=B=L_bf, A=1.0)

struct Uniforms {
    width:   u32,
    height:  u32,
    sigma_s: f32,   // spatial kernel std-dev (pixels)
    sigma_r: f32,   // range kernel std-dev (luma [0,1])
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var input_tex: texture_2d<f32>;
@group(0) @binding(2) var luma_tex:  texture_storage_2d<rgba8unorm, write>;

// In Rgba8Unorm texture with [B, G, R, A] byte layout: c.r=B, c.g=G, c.b=R
// ITU-R BT.709 sRGB luminance: Y = 0.2126*R + 0.7152*G + 0.0722*B
fn luma(c: vec4<f32>) -> f32 {
    return dot(c.rgb, vec3<f32>(0.0722, 0.7152, 0.2126));
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;
    if (x >= u.width || y >= u.height) { return; }

    let center = vec2<i32>(i32(x), i32(y));
    let L_p = luma(textureLoad(input_tex, center, 0));

    let r = min(i32(ceil(2.5 * u.sigma_s)), 3);
    let inv_2ss = -1.0 / (2.0 * u.sigma_s * u.sigma_s);
    let inv_2sr = -1.0 / (2.0 * u.sigma_r * u.sigma_r);

    var weighted_sum: f32 = 0.0;
    var weight_total: f32 = 0.0;

    for (var dy: i32 = -r; dy <= r; dy++) {
        let ny = clamp(center.y + dy, 0, i32(u.height) - 1);
        for (var dx: i32 = -r; dx <= r; dx++) {
            let nx = clamp(center.x + dx, 0, i32(u.width) - 1);
            let L_q = luma(textureLoad(input_tex, vec2<i32>(nx, ny), 0));

            let dist_sq = f32(dx * dx + dy * dy);
            let range_diff = L_q - L_p;

            let g_s = exp(dist_sq * inv_2ss);
            let f_r = exp(range_diff * range_diff * inv_2sr);
            let w   = g_s * f_r;

            weighted_sum += L_q * w;
            weight_total += w;
        }
    }

    let L_bf = weighted_sum / max(weight_total, 1e-6);
    textureStore(luma_tex, center, vec4<f32>(L_bf, L_bf, L_bf, 1.0));
}
