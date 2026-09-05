// VACT V4 — Pass 3: Morphological Opening + RGBA Heatmap
//
// Applies morphological OPENING (erosion followed by dilation) to the Sobel
// gradient magnitude texture. Opening suppresses isolated single-pixel noise
// spikes while preserving continuous UI element edge contours.
//
// Additionally writes a colour heatmap RGBA8 output for debug visualization:
//   low magnitude  → deep blue (0,0,128)
//   mid magnitude  → green     (0,255,0)
//   high magnitude → hot red   (255,0,0)
//
// Input:  grad_tex   — R32Float Sobel magnitude (Pass 2 output)
// Output: final_tex  — R32Float opened gradient magnitude (for V5 CCL input)
//         heat_tex   — Rgba8Unorm heatmap (debug PNG)
//
// Implemented as two chained workgroup-size dispatches in a single WGSL shader
// file with two entry points (one per sub-pass), called from Rust sequentially.

struct Uniforms {
    width:   u32,
    height:  u32,
    morph_r: u32,   // structuring element half-radius (default 1 = 3x3 SE)
    _pad:    u32,
};

// ─── Entry point A: Erosion ──────────────────────────────────────────────────

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var grad_tex:   texture_2d<f32>;
@group(0) @binding(2) var erode_tex:  texture_storage_2d<r32float, write>;

@compute @workgroup_size(16, 16, 1)
fn erode(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (u32(x) >= u.width || u32(y) >= u.height) { return; }

    let r = i32(u.morph_r);
    var minimum: f32 = 1.0;

    for (var dy: i32 = -r; dy <= r; dy++) {
        for (var dx: i32 = -r; dx <= r; dx++) {
            let nx = clamp(x + dx, 0, i32(u.width)  - 1);
            let ny = clamp(y + dy, 0, i32(u.height) - 1);
            let v = textureLoad(grad_tex, vec2<i32>(nx, ny), 0).r;
            minimum = min(minimum, v);
        }
    }

    textureStore(erode_tex, vec2<i32>(x, y), vec4<f32>(minimum, 0.0, 0.0, 0.0));
}

// ─── Entry point B: Dilation + Heatmap ───────────────────────────────────────

@group(1) @binding(0) var<uniform> u2: Uniforms;
@group(1) @binding(1) var erode_tex2: texture_2d<f32>;
@group(1) @binding(2) var final_tex:  texture_storage_2d<r32float, write>;
@group(1) @binding(3) var heat_tex:   texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16, 1)
fn dilate_and_heat(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (u32(x) >= u2.width || u32(y) >= u2.height) { return; }

    let r = i32(u2.morph_r);
    var maximum: f32 = 0.0;

    for (var dy: i32 = -r; dy <= r; dy++) {
        for (var dx: i32 = -r; dx <= r; dx++) {
            let nx = clamp(x + dx, 0, i32(u2.width)  - 1);
            let ny = clamp(y + dy, 0, i32(u2.height) - 1);
            let v = textureLoad(erode_tex2, vec2<i32>(nx, ny), 0).r;
            maximum = max(maximum, v);
        }
    }

    textureStore(final_tex, vec2<i32>(x, y), vec4<f32>(maximum, 0.0, 0.0, 0.0));

    // Heatmap: blue→green→red across [0,1]
    var color: vec4<f32>;
    if (maximum < 0.5) {
        // blue → green
        let t = maximum * 2.0;
        color = vec4<f32>(0.0, t, 1.0 - t, 1.0);
    } else {
        // green → red
        let t = (maximum - 0.5) * 2.0;
        color = vec4<f32>(t, 1.0 - t, 0.0, 1.0);
    }
    textureStore(heat_tex, vec2<i32>(x, y), color);
}
