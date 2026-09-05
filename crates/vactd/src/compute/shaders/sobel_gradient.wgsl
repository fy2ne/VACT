// VACT V4/V5 — Pass 2: Sobel Gradient Magnitude (Universal RGBA8)
//
// Computes 3×3 Sobel edge gradient magnitude from smoothed luminance.
// Input:  luma_tex — Rgba8Unorm smoothed luminance (Pass 1 output)
// Output: grad_tex — Rgba8Unorm gradient magnitude (R=G=B=G, A=1.0)

struct Uniforms {
    width:   u32,
    height:  u32,
    sigma_s: f32,
    sigma_r: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var luma_tex: texture_2d<f32>;
@group(0) @binding(2) var grad_tex: texture_storage_2d<rgba8unorm, write>;

fn load(x: i32, y: i32) -> f32 {
    let cx = clamp(x, 0, i32(u.width)  - 1);
    let cy = clamp(y, 0, i32(u.height) - 1);
    return textureLoad(luma_tex, vec2<i32>(cx, cy), 0).r;
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (u32(x) >= u.width || u32(y) >= u.height) { return; }

    // 3×3 neighborhood
    let tl = load(x-1, y-1); let tc = load(x, y-1); let tr = load(x+1, y-1);
    let ml = load(x-1, y  );                         let mr = load(x+1, y  );
    let bl = load(x-1, y+1); let bc = load(x, y+1); let br = load(x+1, y+1);

    // Sobel Gx and Gy operators
    let gx = (tr + 2.0 * mr + br) - (tl + 2.0 * ml + bl);
    let gy = (bl + 2.0 * bc + br) - (tl + 2.0 * tc + tr);

    // Magnitude normalized by 4.0
    let magnitude = clamp(sqrt(gx * gx + gy * gy) / 4.0, 0.0, 1.0);

    textureStore(grad_tex, vec2<i32>(x, y), vec4<f32>(magnitude, magnitude, magnitude, 1.0));
}
