// VACT V4/V5 — Pass 3: Morphological Edge Dilation & Heatmap (Universal RGBA8)
//
// Bridges anti-aliased edge gaps with a 3×3 dilation filter.
// Writes the RGBA8 debug heatmap with gradient magnitude encoded in alpha.
//
// Input:  grad_tex  — Rgba8Unorm Sobel gradient (Pass 2 output)
// Output: final_tex — Rgba8Unorm dilated edge map (R=G=B=maximum, A=1.0)
//         heat_tex  — Rgba8Unorm heatmap (RGB=color, A=maximum for CCL extraction)

struct Uniforms {
    width:   u32,
    height:  u32,
    morph_r: u32,
    _pad:    u32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var grad_tex:  texture_2d<f32>;
@group(0) @binding(2) var final_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var heat_tex:  texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (u32(x) >= u.width || u32(y) >= u.height) { return; }

    let r = i32(u.morph_r);
    var maximum: f32 = 0.0;

    for (var dy: i32 = -r; dy <= r; dy++) {
        let ny = clamp(y + dy, 0, i32(u.height) - 1);
        for (var dx: i32 = -r; dx <= r; dx++) {
            let nx = clamp(x + dx, 0, i32(u.width) - 1);
            let v = textureLoad(grad_tex, vec2<i32>(nx, ny), 0).r;
            maximum = max(maximum, v);
        }
    }

    textureStore(final_tex, vec2<i32>(x, y), vec4<f32>(maximum, maximum, maximum, 1.0));

    // Dynamic heatmap RGB + Alpha carries normalized gradient magnitude [0.0, 1.0]
    var color: vec4<f32>;
    if (maximum < 0.25) {
        let t = maximum * 4.0;
        color = vec4<f32>(0.0, t * 0.5, 0.5 + t * 0.5, maximum);
    } else if (maximum < 0.6) {
        let t = (maximum - 0.25) / 0.35;
        color = vec4<f32>(t, 1.0, 1.0 - t, maximum);
    } else {
        let t = (maximum - 0.6) / 0.4;
        color = vec4<f32>(1.0, 1.0 - t, 0.0, maximum);
    }

    textureStore(heat_tex, vec2<i32>(x, y), color);
}
