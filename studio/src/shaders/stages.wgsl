// Every stage of the simulation is a full-screen pass over the picture. All passes share
// one layout: a block of numbers, a linear sampler that mirrors at the borders (the same
// border rule as scipy's "reflect"), six picture inputs and two tables read by index.

struct Params {
    v: array<vec4<f32>, 24>,
}

@group(0) @binding(0) var<uniform> u: Params;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var t0: texture_2d<f32>;
@group(0) @binding(3) var t1: texture_2d<f32>;
@group(0) @binding(4) var t2: texture_2d<f32>;
@group(0) @binding(5) var t3: texture_2d<f32>;
@group(0) @binding(6) var t4: texture_2d<f32>;
@group(0) @binding(7) var t5: texture_2d<f32>;
@group(0) @binding(8) var l0: texture_2d<f32>;
@group(0) @binding(9) var l1: texture_2d<f32>;

const LN10: f32 = 2.302585093;
const LOG10E: f32 = 0.4342944819;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn lin_to_srgb(x0: f32) -> f32 {
    let x = max(x0, 0.0);
    if (x <= 0.0031308) {
        return x * 12.92;
    }
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}

fn srgb_to_lin(x: f32) -> f32 {
    if (x <= 0.04045) {
        return x / 12.92;
    }
    return pow((x + 0.055) / 1.055, 2.4);
}

// numpy.interp on an evenly spaced table held in l0: (first x, 1 / step, entries)
fn table(x: f32, grid: vec3<f32>) -> f32 {
    let f = clamp((x - grid.x) * grid.y, 0.0, grid.z - 1.0);
    let i = min(i32(f), i32(grid.z) - 2);
    let a = textureLoad(l0, vec2<i32>(i, 0), 0).r;
    let b = textureLoad(l0, vec2<i32>(i + 1, 0), 0).r;
    return mix(a, b, f - f32(i));
}

fn sstep(lo: f32, hi: f32, x: f32) -> f32 {
    if (hi <= lo) {
        return select(0.0, 1.0, x >= hi);
    }
    let t = clamp((x - lo) / (hi - lo), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// ---------- exposure: the photo through the film's colour response -------------------
// v0 = (weight r, g, b, enlargement k)

fn cubic(d: f32) -> f32 {
    let x = abs(d);
    if (x < 1.0) {
        return (1.5 * x - 2.5) * x * x + 1.0;
    }
    if (x < 2.0) {
        return ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0;
    }
    return 0.0;
}

@fragment
fn fs_expose(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let k = u.v[0].w;
    var rgb: vec3<f32>;
    if (k <= 1.0) {
        rgb = textureLoad(t0, vec2<i32>(pos.xy), 0).rgb;
    } else {
        // enlarged with the bicubic kernel Pillow uses, borders included
        let size = vec2<i32>(textureDimensions(t0));
        let c = pos.xy / k;
        let b = vec2<i32>(floor(c - 0.5));
        var acc = vec3<f32>(0.0);
        var wsum = 0.0;
        for (var j = -1; j <= 2; j++) {
            for (var i = -1; i <= 2; i++) {
                let q = b + vec2<i32>(i, j);
                if (q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y) {
                    continue;
                }
                let w = cubic(c.x - (f32(q.x) + 0.5)) * cubic(c.y - (f32(q.y) + 0.5));
                acc += w * textureLoad(t0, q, 0).rgb;
                wsum += w;
            }
        }
        rgb = max(acc / wsum, vec3<f32>(0.0));
    }
    return vec4<f32>(dot(u.v[0].xyz, rgb), 0.0, 0.0, 1.0);
}

// ---------- one axis of a Gaussian blur ---------------------------------------------
// v0 = (1 / width, 1 / height, pairs, centre weight), v1 = (direction x, y)
// v2.. = (offset, weight) of each pair of neighbouring taps, two pairs per entry.
// A pair is read with one filtered sample placed between its two taps.

@fragment
fn fs_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let inv = u.v[0].xy;
    let uv = pos.xy * inv;
    let dir = u.v[1].xy * inv;
    let n = i32(u.v[0].z);
    var acc = textureSampleLevel(t0, samp, uv, 0.0) * u.v[0].w;
    for (var i = 0; i < n; i++) {
        let q = u.v[2 + i / 2];
        let ow = select(q.xy, q.zw, (i & 1) == 1);
        let d = dir * ow.x;
        acc += (textureSampleLevel(t0, samp, uv + d, 0.0) + textureSampleLevel(t0, samp, uv - d, 0.0)) * ow.y;
    }
    return acc;
}

// ---------- half-size copy, filter (1 3 3 1) / 8 on each axis -----------------------
// v0 = (1 / source width, 1 / source height)

@fragment
fn fs_down(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let inv = u.v[0].xy;
    let c = floor(pos.xy) * 2.0 + 1.0;
    let o = 0.75;
    return 0.25 * (textureSampleLevel(t0, samp, (c + vec2<f32>(-o, -o)) * inv, 0.0)
        + textureSampleLevel(t0, samp, (c + vec2<f32>(o, -o)) * inv, 0.0)
        + textureSampleLevel(t0, samp, (c + vec2<f32>(-o, o)) * inv, 0.0)
        + textureSampleLevel(t0, samp, (c + vec2<f32>(o, o)) * inv, 0.0));
}

// ---------- weighted sum of up to six pictures, reduced ones enlarged ---------------
// v0 = (count), v1..v6 = (texture coordinate per output pixel x, y, weight)

@fragment
fn fs_combine(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let n = i32(u.v[0].x);
    var acc = u.v[1].z * textureSampleLevel(t0, samp, pos.xy * u.v[1].xy, 0.0);
    if (n > 1) {
        acc += u.v[2].z * textureSampleLevel(t1, samp, pos.xy * u.v[2].xy, 0.0);
    }
    if (n > 2) {
        acc += u.v[3].z * textureSampleLevel(t2, samp, pos.xy * u.v[3].xy, 0.0);
    }
    if (n > 3) {
        acc += u.v[4].z * textureSampleLevel(t3, samp, pos.xy * u.v[4].xy, 0.0);
    }
    if (n > 4) {
        acc += u.v[5].z * textureSampleLevel(t4, samp, pos.xy * u.v[5].xy, 0.0);
    }
    if (n > 5) {
        acc += u.v[6].z * textureSampleLevel(t5, samp, pos.xy * u.v[6].xy, 0.0);
    }
    return acc;
}

// ---------- development: exposure to density, with edge effects ---------------------
// v0 = (exposure gain, log exposure of a metered grey, base density, pass: 0 plain, 1 edges)
// v1 = (curve first x, 1 / step, entries, edge strength k)
// v2 = (tail weight, growth with density, density of a metered grey)
// v3 = (texture coordinate per pixel of the two spread pictures)
// t0 exposure, t1 density of the plain pass, t2 and t3 that density spread out, l0 the curve

fn film_density(logh: f32) -> f32 {
    return table(logh, u.v[1].xyz) - u.v[0].z;
}

@fragment
fn fs_develop(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    let e = textureLoad(t0, p, 0).r * u.v[0].x;
    var logh = log(max(e, 1e-6) / 0.18) * LOG10E + u.v[0].y;
    if (u.v[0].w > 0.5) {
        let d = textureLoad(t1, p, 0).r;
        let tw = u.v[2].x;
        let spread = (1.0 - tw) * textureSampleLevel(t2, samp, pos.xy * u.v[3].xy, 0.0).r
            + tw * textureSampleLevel(t3, samp, pos.xy * u.v[3].zw, 0.0).r;
        var k = u.v[1].w;
        if (u.v[2].y > 0.0) {
            k *= pow(max(d, 1e-6) / u.v[2].z, u.v[2].y);
        }
        logh += k * (d - spread);
    }
    return vec4<f32>(film_density(logh), 0.0, 0.0, 1.0);
}

// ---------- grain: developed grains per pixel are Poisson ---------------------------
// v0 = (grains per pixel, seed). Output: transmission of the negative, with grain and without.

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

// a number in (0, 1) fixed for each pixel and seed
fn rand(p: vec2<u32>, seed: u32) -> f32 {
    let h = pcg(p.x + pcg(p.y + pcg(seed)));
    return (f32(h >> 8u) + 0.5) / 16777216.0;
}

// inverse of the normal distribution (Acklam's rational approximation)
fn inv_norm(p: f32) -> f32 {
    if (p < 0.02425 || p > 0.97575) {
        let pp = min(p, 1.0 - p);
        let q = sqrt(-2.0 * log(pp));
        let x = (((((-0.007784894002 * q - 0.3223964580) * q - 2.400758277) * q - 2.549732539) * q + 4.374664141) * q + 2.938163983)
            / ((((0.007784695709 * q + 0.3224671291) * q + 2.445134137) * q + 3.754408662) * q + 1.0);
        return select(-x, x, p < 0.5);
    }
    let q = p - 0.5;
    let r = q * q;
    return (((((-39.69683029 * r + 220.9460984) * r - 275.9285104) * r + 138.3577519) * r - 30.66479807) * r + 2.506628277) * q
        / (((((-54.47609880 * r + 161.5858369) * r - 155.6989799) * r + 66.80131189) * r - 13.28068155) * r + 1.0);
}

// Poisson count with mean `lam` at the quantile `q`. The same q gives a count that moves
// smoothly with lam, so the grain pattern stays put while a slider is dragged.
fn poisson(lam: f32, q: f32) -> f32 {
    if (lam <= 0.0) {
        return 0.0;
    }
    if (lam < 12.0) {
        var p = exp(-lam);
        var c = p;
        var k = 0.0;
        for (var i = 0; i < 64; i++) {
            if (q <= c) {
                break;
            }
            k += 1.0;
            p *= lam / k;
            c += p;
        }
        return k;
    }
    // normal quantile with corrections for the skew of the Poisson distribution
    let z = inv_norm(q);
    let s = sqrt(lam);
    return max(0.0, floor(lam + s * z + (z * z - 1.0) / 6.0 + (z - z * z * z) / (72.0 * s) + 0.5));
}

@fragment
fn fs_grain(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let d0 = textureLoad(t0, vec2<i32>(pos.xy), 0).r;
    var d = d0;
    let n = u.v[0].x;
    if (n > 0.0) {
        let prob = clamp(d0 / 3.4, 0.0, 0.98);
        let sat = 1.0 - 0.6 * prob;
        let q = rand(vec2<u32>(pos.xy), u32(u.v[0].y));
        d = poisson(n * prob / sat, q) * (3.4 / n) * sat;
    }
    return vec4<f32>(exp(-d * LN10), exp(-d0 * LN10), 0.0, 1.0);
}

// ---------- the print: negative transmission to paper reflectance -------------------
// v0 = (exposure offset, paper white density, grain kept in light tones)
// v1 = (curve first x, 1 / step, entries). t0 transmission (grain, clean), l0 paper curve

fn print_reflectance(t: f32) -> f32 {
    let dn = -log(max(t, 1e-6)) * LOG10E;
    let dp = table(u.v[0].x - dn, u.v[1].xyz);
    return exp(-(dp - u.v[0].y) * LN10);
}

@fragment
fn fs_paper(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let t = textureLoad(t0, vec2<i32>(pos.xy), 0).rg;
    let r = print_reflectance(t.r);
    let keep_light = u.v[0].z;
    if (keep_light >= 1.0) {
        return vec4<f32>(r, 0.0, 0.0, 1.0);
    }
    // grain is the difference from the same print made without it, scaled down in light tones
    let v0 = lin_to_srgb(print_reflectance(t.g));
    let keep = 1.0 - (1.0 - keep_light) * sstep(0.35, 0.55, v0);
    let v = clamp(v0 + keep * (lin_to_srgb(r) - v0), 0.0, 1.0);
    return vec4<f32>(srgb_to_lin(v), 0.0, 0.0, 1.0);
}

// ---------- tone steps of the copy, the house range and the press -------------------
// v0 = (step, print black, solid ink, tau of the range compression)
// v1 = (density range in, range out, dot that drops out, tone that fills in)
// v2 = (copy contrast, smallest dot, dot gain, pooling factor), v3 = (table entries)

fn compress(d: f32) -> f32 {
    let tau = u.v[0].w;
    if (tau <= 0.0) {
        return d;
    }
    return u.v[1].y * (1.0 - exp(-d / tau)) / (1.0 - exp(-u.v[1].x / tau));
}

fn printed_once(t_in: f32) -> f32 {
    let drop = u.v[1].z;
    let fill = u.v[1].w;
    let t = t_in * sstep(0.6 * drop, 1.4 * drop, t_in);
    return 1.0 - (1.0 - t) * sstep(0.6 * (1.0 - fill), 1.4 * (1.0 - fill), 1.0 - t);
}

fn s_curve(x_in: f32, gain: f32) -> f32 {
    let x = clamp(x_in, 0.0, 1.0);
    if (x < 0.5) {
        return 0.5 * pow(max(2.0 * x, 1e-9), gain);
    }
    return 1.0 - 0.5 * pow(max(2.0 * (1.0 - x), 1e-9), gain);
}

@fragment
fn fs_tone(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let step = i32(u.v[0].x);
    let black = u.v[0].y;
    let solid = u.v[0].z;
    let p = vec2<i32>(pos.xy);
    var o = 0.0;
    if (step == 1) {
        // first printing: the print's density range fitted to what ink can hold
        let r = textureLoad(t0, p, 0).r;
        let d = compress(-log(clamp(r, black, 1.0)) * LOG10E);
        o = printed_once((1.0 - exp(-d * LN10)) / (1.0 - solid));
    } else if (step == 2) {
        // contrasty copy film, then the second printing
        let t = textureLoad(t0, p, 0).r;
        let vb = lin_to_srgb(black);
        var v = (lin_to_srgb(1.0 - t * (1.0 - black)) - vb) / (1.0 - vb);
        v = vb + s_curve(v, u.v[2].x) * (1.0 - vb);
        let t2 = printed_once(clamp((1.0 - srgb_to_lin(v)) / (1.0 - black), 0.0, 1.0));
        o = 1.0 - t2 * (1.0 - black);
    } else if (step == 3) {
        // every third pixel as a display value, for the picture's percentiles
        o = lin_to_srgb(clamp(textureLoad(t0, p * 3, 0).r, 0.0, 1.0));
    } else if (step == 4) {
        // the house tone curve, as a table from display value to reflectance
        let v = lin_to_srgb(clamp(textureLoad(t0, p, 0).r, 0.0, 1.0));
        o = table(v, vec3<f32>(0.0, u.v[3].x - 1.0, u.v[3].x));
    } else if (step == 5) {
        // the press: ink coverage asked for
        let r = textureLoad(t0, p, 0).r;
        let d = compress(-log(clamp(r, 1e-4, 1.0)) * LOG10E);
        o = (1.0 - exp(-d * LN10)) / (1.0 - solid);
    } else if (step == 6) {
        // smallest dots fail, dots spread
        var a = textureLoad(t0, p, 0).r;
        let min_dot = u.v[2].y;
        if (min_dot > 0.0 && a < min_dot) {
            a = a * (a / min_dot) * (a / min_dot);
        }
        a = clamp(a + 4.0 * u.v[2].z * a * (1.0 - a), 0.0, 1.0);
        o = 1.0 - a * (1.0 - solid);
    } else {
        // average k x k cells back to the picture's own grid
        let k = i32(u.v[2].w);
        for (var j = 0; j < k; j++) {
            for (var i = 0; i < k; i++) {
                o += textureLoad(t0, p * k + vec2<i32>(i, j), 0).r;
            }
        }
        o = o / f32(k * k);
    }
    return vec4<f32>(o, 0.0, 0.0, 1.0);
}

// ---------- half-size average for the display pyramid -------------------------------
// v0 = (1 / source width, 1 / source height)

@fragment
fn fs_mip(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return textureSampleLevel(t0, samp, (floor(pos.xy) * 2.0 + 1.0) * u.v[0].xy, 0.0);
}

// ---------- the canvas: the finished picture at screen size --------------------------
// v0 = (origin x, y in picture pixels, picture pixels per screen pixel, pyramid level)
// v1 = (view: 0 print, 1 scan, 2 page; encoded solid ink; scan black; scan white)
// v2 = (paper colour r, g, b; dither strength)
// v3 = (first canvas column showing the photo itself, 1 to write display values
//       unconverted, the column after the last showing it, 1 to mark clipped values)
// t0 the finished picture with its pyramid, t1 the photo

fn texel(p: vec2<i32>, lvl: i32, src: bool) -> vec3<f32> {
    if (src) {
        let size = vec2<i32>(textureDimensions(t1));
        return textureLoad(t1, clamp(p, vec2<i32>(0), size - 1), 0).rgb;
    }
    let size = vec2<i32>(textureDimensions(t0, lvl));
    return vec3<f32>(textureLoad(t0, clamp(p, vec2<i32>(0), size - 1), lvl).r);
}

fn sample_picture(pos: vec2<f32>, src: bool) -> vec3<f32> {
    let s = u.v[0].z;
    let c = u.v[0].xy + pos * s;
    if (s <= 0.5) {
        return texel(vec2<i32>(floor(c)), 0, src);          // enlarged twice or more: show the pixels
    }
    if (s <= 1.0) {
        let b = vec2<i32>(floor(c - 0.5));
        var acc = vec3<f32>(0.0);
        for (var j = -1; j <= 2; j++) {
            for (var i = -1; i <= 2; i++) {
                let q = b + vec2<i32>(i, j);
                acc += cubic(c.x - (f32(q.x) + 0.5)) * cubic(c.y - (f32(q.y) + 0.5)) * texel(q, 0, src);
            }
        }
        return max(acc, vec3<f32>(0.0));
    }
    // reduced: average the picture pixels each screen pixel covers, in linear light
    let lvl = i32(u.v[0].w);
    let f = exp2(f32(lvl));
    let lo = (c - 0.5 * s) / f;
    let hi = (c + 0.5 * s) / f;
    let i0 = vec2<i32>(floor(lo));
    let i1 = min(vec2<i32>(ceil(hi)) - 1, i0 + 7);
    var acc = vec3<f32>(0.0);
    var ws = 0.0;
    for (var y = i0.y; y <= i1.y; y++) {
        let wy = min(hi.y, f32(y + 1)) - max(lo.y, f32(y));
        for (var x = i0.x; x <= i1.x; x++) {
            let w = wy * (min(hi.x, f32(x + 1)) - max(lo.x, f32(x)));
            acc += w * texel(vec2<i32>(x, y), lvl, src);
            ws += w;
        }
    }
    return acc / max(ws, 1e-6);
}

@fragment
fn fs_canvas(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let src = pos.x >= u.v[3].x && pos.x < u.v[3].z;
    let r = sample_picture(pos.xy, src);
    var enc: vec3<f32>;
    if (src) {
        enc = vec3<f32>(lin_to_srgb(r.r), lin_to_srgb(r.g), lin_to_srgb(r.b));
    } else if (u.v[1].x > 1.5) {
        let c = clamp(r.r, 0.0, 1.0) * u.v[2].rgb;
        enc = vec3<f32>(lin_to_srgb(c.r), lin_to_srgb(c.g), lin_to_srgb(c.b));
    } else if (u.v[1].x > 0.5) {
        let ek = u.v[1].y;
        enc = vec3<f32>(clamp(u.v[1].z + (lin_to_srgb(r.r) - ek) / (u.v[1].w - ek) * (1.0 - u.v[1].z), 0.0, 1.0));
    } else {
        enc = vec3<f32>(lin_to_srgb(clamp(r.r, 0.0, 1.0)));
    }
    if (u.v[3].w > 0.5) {
        if (min(enc.r, min(enc.g, enc.b)) >= 0.998) {
            enc = vec3<f32>(1.0, 0.25, 0.2);
        } else if (max(enc.r, max(enc.g, enc.b)) <= 0.002) {
            enc = vec3<f32>(0.2, 0.45, 1.0);
        }
    }
    if (u.v[3].y > 0.5) {
        return vec4<f32>(enc, 1.0);
    }
    // a little noise below one display step keeps smooth tones from banding in 8 bits
    let n = (rand(vec2<u32>(pos.xy), 7u) - 0.5) * u.v[2].w / 255.0;
    let e = clamp(enc + n, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(srgb_to_lin(e.r), srgb_to_lin(e.g), srgb_to_lin(e.b), 1.0);
}
