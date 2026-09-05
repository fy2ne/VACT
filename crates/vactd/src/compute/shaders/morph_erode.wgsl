// VACT V4/V5 — Pass 3b: Morphological Erosion + RGBA Heatmap
//
// Completes morphological closing (dilation then erosion) by eroding the dilated
// edge magnitude back to crisp boundary contours while generating the RGBA heatmap.
//
// Input:  dilate_tex — R32Float dilated magnitude (Pass 3a output)
// Output: final_tex  — R32Float closed gradient (fed into V5 CCL)
//         heat_tex   — Rgba8Unorm heatmap (debug visualization)

struct Uniforms {
    width:   u32,
    height:  u32,
    morph_r: u32,
    _pad:    u32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var dilate_tex: texture_2d<f32>;
@group(0) @binding(2) var final_tex:  texture_storage_2d<r32float,    write>;
@group(0) @binding(3) var heat_tex:   texture_storage_2d<rgba8unorm,  write>;

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (u32(x) >= u.width || u32(y) >= u.height) { return; }

    let r = i32(u.morph_r);
    var minimum: f32 = 1.0;

    for (var dy: i32 = -r; dy <= r; dy++) {
        let ny = clamp(y + dy, 0, i32(u.height) - 1);
        for (var dx: i32 = -r; dx <= r; dx++) {
            let nx = clamp(x + dx, 0, i32(u.width) - 1);
            let v = textureLoad(dilate_tex, vec2<i32>(nx, ny), 0).r;
            minimum = min(minimum, v);
        }
    }

    textureStore(final_tex, vec2<i32>(x, y), vec4<f32>(minimum, 0.0, 0.0, 0.0));

    // Dynamic heatmap: blue (0) → green (0.3) → yellow (0.6) → red (1.0)
    var color: vec4<f32>;
    if (minimum < 0.5) {
        let t = minimum * 2.0;
        color = vec4<f32>(0.0, t, 1.0 - t, 1.0);
    } else {
        let t = (minimum - 0.5) * 2.0;
        color = vec4<f32>(t, 1.0 - t, 0.0, 1.0);
    }
    textureStore(heat_tex, vec2<i32>(x, y), color);
}
