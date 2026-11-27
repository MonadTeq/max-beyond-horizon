#version 450

// Fullscreen triangle for the FXAA pass (GPU anti-aliasing, `gpu-aa-design.md`).
//
// One oversized triangle covers the whole render target: the three vertices are
// (0,0), (2,0) and (0,2) in UV space, so the clip-space triangle is
// (-1,+1), (+3,+1), (-1,-3). The pass needs no vertex buffer, which is why the
// graphics pipeline is created with an empty vertex input state and only
// `draw_primitives(3, 1, 0, 0)`.
//
// SDL3 GPU flips the Vulkan viewport (`SDL_gpu_vulkan.c`: `viewport.height =
// -viewport.h`) to match D3D12/Metal, so clip-space `y = +1` is the TOP of the
// target. The UV space of the fullscreen triangle is therefore mapped with an
// inverted y: `v = 0` stays the top row of the source texture, exactly as the
// frame was uploaded, and the pass is orientation-preserving.

layout(location = 0) out vec2 v_uv;

void main() {
    vec2 uv = vec2(float((gl_VertexIndex << 1) & 2), float(gl_VertexIndex & 2));

    v_uv = uv;
    // `y = +1` is the top of the render target on SDL3 GPU (viewport flip).
    gl_Position = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
}
