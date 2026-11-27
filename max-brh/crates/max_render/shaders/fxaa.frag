#version 450

// FXAA (single pass) for the M.A.X. remaster GPU anti-aliasing
// (`max-brh-docs/development/gpu-aa-design.md`).
//
// The pass runs at the resolution of the render frame (`640·S × 480·S`), so the
// blit that follows scales the *anti-aliased* frame to the window — anti-aliasing
// is never applied after the upscale.
//
// Algorithm: the classic 5-tap FXAA edge blur (direction from the four diagonal
// luma taps, span clamp, the A/B blend with the luma-range fallback), with the
// flat-area early-out of Lottes' FXAA 3.11 added: a neighbourhood whose luma
// range is below `threshold` is copied verbatim, so flat regions — sprite
// interiors, tile fills, UI chrome — stay byte-identical.
//
// Resource layout is the SDL3 GPU SPIR-V convention for graphics pipelines:
// set 0/1 = vertex stage (samplers/uniforms), set 2/3 = fragment stage. The
// fragment sampler therefore lives in set 2 and the uniform block in set 3, and
// the uniform is filled by `SDL_PushGPUFragmentUniformData(slot 0)`.
//
// Derived from the compact FXAA shader by Armin Ronacher (2011), itself based on
// the code from geeks3d.com:
//
//   Copyright (c) 2011 by Armin Ronacher. All rights reserved.
//   Redistribution and use in source and binary forms, with or without
//   modification, are permitted provided that the following conditions are met:
//     * Redistributions of source code must retain the above copyright notice,
//       this list of conditions and the following disclaimer.
//     * Redistributions in binary form must reproduce the above copyright
//       notice, this list of conditions and the following disclaimer in the
//       documentation and/or other materials provided with the distribution.
//     * The names of the contributors may not be used to endorse or promote
//       products derived from this software without specific prior written
//       permission.
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
//   AND ANY EXPRESS OR IMPLIED WARRANTIES ARE DISCLAIMED.

layout(set = 2, binding = 0) uniform sampler2D u_source;

layout(set = 3, binding = 0) uniform FxaaParams {
    // Luma contrast below which a neighbourhood counts as flat (early-out).
    float threshold;
    // Padding to the 16-byte std140 block size.
    vec3 padding;
} u_params;

layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 o_color;

const vec3 LUMA = vec3(0.299, 0.587, 0.114);
const float REDUCE_MIN = 1.0 / 128.0;
const float REDUCE_MUL = 1.0 / 8.0;
const float SPAN_MAX = 8.0;

// One texel step in UV space, taken from the source itself: the pass must not
// depend on a pushed uniform to be *geometrically* correct (a zero texel size
// silently collapses every tap onto the centre pixel and the pass becomes a
// copy).
vec2 texel_size() {
    return 1.0 / vec2(textureSize(u_source, 0));
}

vec3 sample_at(vec2 offset) {
    return texture(u_source, v_uv + offset * texel_size()).rgb;
}

void main() {
    vec3 rgbM = sample_at(vec2(0.0, 0.0));
    vec3 rgbNW = sample_at(vec2(-1.0, -1.0));
    vec3 rgbNE = sample_at(vec2(1.0, -1.0));
    vec3 rgbSW = sample_at(vec2(-1.0, 1.0));
    vec3 rgbSE = sample_at(vec2(1.0, 1.0));

    float lumaM = dot(rgbM, LUMA);
    float lumaNW = dot(rgbNW, LUMA);
    float lumaNE = dot(rgbNE, LUMA);
    float lumaSW = dot(rgbSW, LUMA);
    float lumaSE = dot(rgbSE, LUMA);

    float lumaMin = min(lumaM, min(min(lumaNW, lumaNE), min(lumaSW, lumaSE)));
    float lumaMax = max(lumaM, max(max(lumaNW, lumaNE), max(lumaSW, lumaSE)));

    // Flat neighbourhood: no edge to smooth, copy the source texel untouched.
    if ((lumaMax - lumaMin) < u_params.threshold) {
        o_color = vec4(rgbM, 1.0);
        return;
    }

    // Direction of the edge, from the four diagonal luma taps.
    vec2 dir;
    dir.x = -((lumaNW + lumaNE) - (lumaSW + lumaSE));
    dir.y = ((lumaNW + lumaSW) - (lumaNE + lumaSE));

    float dirReduce = max((lumaNW + lumaNE + lumaSW + lumaSE) * (0.25 * REDUCE_MUL), REDUCE_MIN);
    float rcpDirMin = 1.0 / (min(abs(dir.x), abs(dir.y)) + dirReduce);
    // `dir` stays in **texel** units: `sample_at` converts an offset to UV with
    // the texel size, so multiplying here as well would shrink every tap into
    // the centre texel and turn the whole pass into a copy.
    dir = clamp(dir * rcpDirMin, vec2(-SPAN_MAX), vec2(SPAN_MAX));

    // Blend the two texels straddling the edge (A) and the wider pair (B).
    vec3 rgbA = 0.5 * (sample_at(dir * (1.0 / 3.0 - 0.5)) + sample_at(dir * (2.0 / 3.0 - 0.5)));
    vec3 rgbB = rgbA * 0.5 + 0.25 * (sample_at(dir * -0.5) + sample_at(dir * 0.5));

    float lumaB = dot(rgbB, LUMA);

    // A blend outside the local luma range is a bleeding artefact: fall back to A.
    if ((lumaB < lumaMin) || (lumaB > lumaMax)) {
        o_color = vec4(rgbA, 1.0);
    } else {
        o_color = vec4(rgbB, 1.0);
    }
}
