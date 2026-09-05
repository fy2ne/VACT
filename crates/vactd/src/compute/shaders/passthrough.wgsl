// VACT V3 — GPU Compute Passthrough Shader
//
// Processes a 2D desktop frame in parallel using 16x16 workgroup tiles.
// Reads from the ingested input texture and writes directly into the output
// storage texture.

struct Uniforms {
    width: u32,
    height: u32,
    flags: u32,
    _padding: u32,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var input_tex: texture_2d<f32>;
@group(0) @binding(2) var output_tex: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    // Bounds check to handle dimensions that are not multiples of 16
    if (x >= uniforms.width || y >= uniforms.height) {
        return;
    }

    let coords = vec2<i32>(i32(x), i32(y));
    let color = textureLoad(input_tex, coords, 0);

    // Passthrough: write input color directly into output storage texture
    textureStore(output_tex, coords, color);
}
