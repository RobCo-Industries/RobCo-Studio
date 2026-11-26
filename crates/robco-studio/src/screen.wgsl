// SPDX-License-Identifier: GPL-3.0-or-later
//
// The screen pass. One full-screen triangle per pass:
// fs_bright and the two blurs make the bloom at half size, fs_comp lays
// every effect over the window's picture, fs_blit draws the result into
// the window. Each strength is 0 to 1, and 0 skips its work.
//
// res: width and height in pixels, time in seconds, pixels per point
// a:   bloom, vignette, scanlines, curvature already scaled
// b:   persistence kept per frame, noise, flicker, shadow mask
// c:   aberration, rolling band, scanline offset in points, unused

struct P {
    res: vec4<f32>,
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
};

@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var t0: texture_2d<f32>;
@group(0) @binding(3) var t1: texture_2d<f32>;
@group(0) @binding(4) var t2: texture_2d<f32>;

struct VO {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VO {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: VO;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

// The barrel curve: from a place on the glass to the place in the picture
// drawn there. Keep in step with `warp` in screen.rs.
fn curve(uv: vec2<f32>, k: f32) -> vec2<f32> {
    var c = uv * 2.0 - 1.0;
    c = c * (1.0 + k * vec2<f32>(c.y * c.y, c.x * c.x));
    return c * 0.5 + 0.5;
}

fn hash(q: vec2<f32>) -> f32 {
    return fract(sin(dot(q, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn fs_bright(in: VO) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(t0, samp, in.uv, 0.0).rgb;
    let l = max(c.r, max(c.g, c.b));
    return vec4<f32>(c * smoothstep(0.3, 0.8, l), 1.0);
}

// Nine taps, spaced by the display scale so the glow keeps its size in points.
fn blur(uv: vec2<f32>, dir: vec2<f32>) -> vec4<f32> {
    let px = dir * p.res.w / vec2<f32>(textureDimensions(t0));
    var w = array<f32, 5>(0.227, 0.194, 0.121, 0.054, 0.016);
    var s = textureSampleLevel(t0, samp, uv, 0.0).rgb * w[0];
    for (var i = 1; i < 5; i++) {
        let o = px * f32(i) * 1.5;
        s += textureSampleLevel(t0, samp, uv + o, 0.0).rgb * w[i];
        s += textureSampleLevel(t0, samp, uv - o, 0.0).rgb * w[i];
    }
    return vec4<f32>(s, 1.0);
}

@fragment
fn fs_blur_h(in: VO) -> @location(0) vec4<f32> {
    return blur(in.uv, vec2<f32>(1.0, 0.0));
}

@fragment
fn fs_blur_v(in: VO) -> @location(0) vec4<f32> {
    return blur(in.uv, vec2<f32>(0.0, 1.0));
}

@fragment
fn fs_comp(in: VO) -> @location(0) vec4<f32> {
    let res = p.res.xy;
    let t = p.res.z;
    let ppp = p.res.w;
    var uv = in.uv;
    if (p.a.w > 0.0) {
        uv = curve(in.uv, p.a.w);
    }
    let inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0));
    if (!inside) {
        // Outside the glass: black, and the trail of nothing.
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    var col: vec3<f32>;
    if (p.c.x > 0.0) {
        // Chromatic aberration: red and blue pulled apart, more at the edges.
        let d = (uv - 0.5) * p.c.x * 0.008;
        col = vec3<f32>(
            textureSampleLevel(t0, samp, uv + d, 0.0).r,
            textureSampleLevel(t0, samp, uv, 0.0).g,
            textureSampleLevel(t0, samp, uv - d, 0.0).b,
        );
    } else {
        col = textureSampleLevel(t0, samp, uv, 0.0).rgb;
    }
    if (p.a.x > 0.0) {
        // Bloom: a blur of the bright pixels, added.
        col += textureSampleLevel(t1, samp, uv, 0.0).rgb * p.a.x * 1.2;
    }
    if (p.a.z > 0.0) {
        // Scanlines: a period of 3 points, moved down by the offset.
        let y = uv.y * res.y / ppp - p.c.z;
        let s = 0.5 + 0.5 * sin(y / 3.0 * 6.2831853);
        col *= 1.0 - p.a.z * 0.45 * (1.0 - s);
    }
    if (p.c.y > 0.0) {
        // A faint bright band that rolls down the screen every 6 seconds.
        let bd = (uv.y - (fract(t / 6.0) * 1.4 - 0.2)) / 0.06;
        col *= 1.0 + p.c.y * 0.15 * exp(-bd * bd);
    }
    if (p.b.w > 0.0) {
        // Shadow mask: red, green and blue columns, one pixel each.
        var mk = vec3<f32>(0.3, 0.3, 0.3);
        let m = u32(in.pos.x) % 3u;
        if (m == 0u) { mk.r = 1.0; } else if (m == 1u) { mk.g = 1.0; } else { mk.b = 1.0; }
        col *= mix(vec3<f32>(1.0), mk * 1.5, p.b.w);
    }
    if (p.b.y > 0.0) {
        // Signal noise: grain that changes every frame.
        col += (hash(in.pos.xy + vec2<f32>(t * 61.0, t * 17.0)) - 0.5) * p.b.y * 0.08;
    }
    if (p.b.z > 0.0) {
        // Flicker: brightness drops by at most 3 percent.
        col *= 1.0 - p.b.z * 0.03 * hash(vec2<f32>(floor(t * 30.0), 1.0));
    }
    if (p.a.y > 0.0) {
        // Vignette: corner shading.
        let v = uv * (1.0 - uv.yx);
        let vig = clamp(pow(max(v.x * v.y * 15.0, 0.0), 0.25), 0.0, 1.0);
        col *= mix(1.0, vig, p.a.y);
    }
    if (p.b.x > 0.0) {
        // Phosphor persistence: the last frame fades instead of vanishing.
        let prev = textureSampleLevel(t2, samp, in.uv, 0.0).rgb;
        col = max(col, prev * p.b.x);
    }
    return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

@fragment
fn fs_blit(in: VO) -> @location(0) vec4<f32> {
    return textureSampleLevel(t0, samp, in.uv, 0.0);
}

// For a window whose surface takes linear colour: the picture is in sRGB.
@fragment
fn fs_blit_linear(in: VO) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(t0, samp, in.uv, 0.0).rgb;
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(hi, lo, c <= vec3<f32>(0.04045)), 1.0);
}
