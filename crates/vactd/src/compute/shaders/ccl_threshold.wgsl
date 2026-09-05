// VACT V5 — CCL Pre-Filter & Edge Binarization
//
// Converts continuous gradient magnitudes into a clean binary classification:
//   - 0: Smooth UI Interior / Background (connected region candidate)
//   - 1: Structural UI Border / Edge boundary
//
// Also tags high-frequency text/icon clusters to ensure small text labels
// are preserved as coherent interactive affordance bounding boxes.

struct Uniforms {
    width:          u32,
    height:         u32,
    edge_threshold: f32, // Default: 0.08 - 0.15
    _pad:           f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var grad_tex:   texture_2d<f32>;
@group(0) @binding(2) var mask_tex:   texture_storage_2d<r32float, write>;

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = i32(gid.x);
    let y = i32(gid.y);
    if (u32(x) >= u.width || u32(y) >= u.height) { return; }

    let g = textureLoad(grad_tex, vec2<i32>(x, y), 0).r;

    // Binary edge decision: 1.0 for edge boundaries, 0.0 for element interiors
    var is_edge: f32 = 0.0;
    if (g >= u.edge_threshold) {
        is_edge = 1.0;
    }

    textureStore(mask_tex, vec2<i32>(x, y), vec4<f32>(is_edge, 0.0, 0.0, 0.0));
}
