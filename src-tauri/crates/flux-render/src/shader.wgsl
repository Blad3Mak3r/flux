struct Out {
    @builtin(position) pos: vec4f,
    @location(0) uvq: vec3f,
    @location(1) local: vec2f,
    @location(2) @interpolate(flat) soft: vec4f,
    @location(3) @interpolate(flat) gamma: f32,
    @location(4) @interpolate(flat) place: vec4f,
    @location(5) @interpolate(flat) opacity: f32,
    @location(6) @interpolate(flat) shading: f32,
    @location(7) @interpolate(flat) fx: f32,
};

@vertex
fn vs(
    @location(0) pos: vec2f,
    @location(1) uvq: vec3f,
    @location(2) local: vec2f,
    @location(3) soft: vec4f,
    @location(4) gamma: f32,
    @location(5) place: vec4f,
    @location(6) opacity: f32,
    @location(7) shading: f32,
    @location(8) fx: f32,
) -> Out {
    var out: Out;
    out.pos = vec4f(pos, 0.0, 1.0);
    out.uvq = uvq;
    out.local = local;
    out.soft = soft;
    out.gamma = gamma;
    out.place = place;
    out.opacity = opacity;
    out.shading = shading;
    out.fx = fx;
    return out;
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

// The window's size and how many masks there are; each mask's first point, its number of points
// and whether it is inverted; the points, two to a line.
struct Masks {
    size: vec4f,
    info: array<vec4u, 8>,
    points: array<vec4f, 128>,
};
@group(1) @binding(0) var<uniform> masks: Masks;

// What is done to each picture, by its place in the layers: how many steps each of the three stages
// has, then the steps (see `effects.rs`, which the CPU monitor mirrors step for step). A step is four
// vec4s: its kind, seed, phase and index; then twelve settings.
struct Fx {
    info: vec4f,
    steps: array<vec4f, 128>,
};
@group(1) @binding(1) var<uniform> effects: array<Fx, 16>;

const TAU: f32 = 6.2831853;
const TAPS: u32 = 32u;
const LUMA: vec3f = vec3f(0.2126, 0.7152, 0.0722);

fn fold(v: f32) -> f32 {
    if v > 0.5 {
        return 1.0 - v;
    }
    return v;
}

fn tap(uv: vec2f) -> vec4f {
    return textureSampleLevel(picture, linear_sampler, uv, 0.0);
}

fn rot(v: vec2f, a: f32) -> vec2f {
    let c = cos(a);
    let s = sin(a);
    return vec2f(c * v.x - s * v.y, s * v.x + c * v.y);
}

// Bounces a coordinate off the edges of the picture.
fn mirror_repeat(v: vec2f) -> vec2f {
    let m = v - 2.0 * floor(v * 0.5);
    return select(m, 2.0 - m, m > vec2f(1.0));
}

fn in_picture(p: vec2f) -> bool {
    return all(p >= vec2f(0.0)) && all(p <= vec2f(1.0));
}

// The "lowbias32" hash; everything random in the effects comes from it, never from a generator.
fn lowbias(x0: u32) -> u32 {
    var x = x0;
    x ^= x >> 16u;
    x *= 0x7feb352du;
    x ^= x >> 15u;
    x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}

fn hash3(a: u32, b: u32, c: u32) -> u32 {
    return lowbias(a ^ lowbias(b ^ lowbias(c)));
}

fn unit(h: u32) -> f32 {
    return f32(h >> 8u) / 16777216.0;
}

fn corner(seed: u32, x: f32, y: f32) -> f32 {
    return unit(hash3(seed, bitcast<u32>(i32(x)), bitcast<u32>(i32(y))));
}

fn value_noise(p: vec2f, seed: u32) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let top = mix(corner(seed, i.x, i.y), corner(seed, i.x + 1.0, i.y), u.x);
    let bottom = mix(corner(seed, i.x, i.y + 1.0), corner(seed, i.x + 1.0, i.y + 1.0), u.x);
    return mix(top, bottom, u.y);
}

// Light to the scale posterize, threshold and levels work on, and back.
fn encode(v: vec3f) -> vec3f {
    return pow(max(v, vec3f(0.0)), vec3f(1.0 / 2.2));
}

fn decode(v: vec3f) -> vec3f {
    return pow(max(v, vec3f(0.0)), vec3f(2.2));
}

// Reads a step's four vec4s: `a`, `b` and `c` are its settings 0-3, 4-7 and 8-11.
fn head(i: u32, k: u32) -> vec4f {
    return effects[i].steps[k * 4u];
}

fn part(i: u32, k: u32, n: u32) -> vec4f {
    return effects[i].steps[k * 4u + 1u + n];
}

// The picture at `at` (0 to 1 across it) with its effects of layer `i`.
fn shade(i: u32, at: vec2f) -> vec4f {
    let n_uv = u32(effects[i].info.x);
    let n_sample = u32(effects[i].info.y);
    let n_colour = u32(effects[i].info.z);
    if n_uv + n_sample + n_colour == 0u {
        return tap(at);
    }
    let size = vec2f(textureDimensions(picture));
    let asp = size.x / size.y;
    let sc = vec2f(asp, 1.0);

    // Where: the geometric steps, walked back from the last to where the picture is read.
    var p = at;
    var seen = 1.0;
    var split = vec2f(0.0);
    for (var k = n_uv; k > 0u; k--) {
        let h = head(i, k - 1u);
        let a = part(i, k - 1u, 0u);
        let kind = u32(h.x);
        let seed = u32(h.y);
        let index = u32(h.w);
        if kind == 0u || kind == 2u {
            p.x = fold(p.x);
        }
        if kind == 1u || kind == 2u {
            p.y = fold(p.y);
        }
        if kind == 3u {
            p = (p - 0.5) / a.x + 0.5;
            if !in_picture(p) {
                seen = 0.0;
                break;
            }
        }
        if kind == 4u {
            p = p - a.xy;
            if a.z > 0.5 {
                p = fract(p);
            } else if !in_picture(p) {
                seen = 0.0;
                break;
            }
        }
        if kind == 5u {
            let q = rot((p - 0.5) * sc, -a.x);
            p = q / sc + 0.5;
            if !in_picture(p) {
                seen = 0.0;
                break;
            }
        }
        if kind == 6u {
            let c = a.zw;
            let q = (p - c) * sc;
            let r = length(q);
            let segment = TAU / a.x;
            var t = atan2(q.y, q.x) - a.y;
            t = t - segment * floor(t / segment);
            t = abs(t - segment * 0.5) + a.y;
            p = mirror_repeat(vec2f(r * cos(t), r * sin(t)) / sc + c);
        }
        if kind == 7u {
            let q = (p - 0.5) * sc;
            let polar = mirror_repeat(vec2f(atan2(q.y, q.x) / TAU + 0.5, length(q) * 2.0));
            p = mix(p, polar, a.x);
        }
        if kind == 8u {
            let o = p;
            if a.w != 1.0 {
                p.x += a.x * sin(TAU * (a.y * o.y + h.z));
            }
            if a.w >= 1.0 {
                p.y += a.x * sin(TAU * (a.y * o.x + h.z));
            }
            p = mirror_repeat(p);
        }
        if kind == 9u {
            let c = a.zw;
            var q = (p - c) * sc;
            let t = clamp(1.0 - length(q) / max(a.y, 0.0001), 0.0, 1.0);
            q = rot(q, a.x * t * t);
            p = mirror_repeat(q / sc + c);
        }
        if kind == 10u {
            let c = a.zw;
            var q = (p - c) * sc;
            let n = length(q) / max(a.y, 0.0001);
            if n < 1.0 {
                q = q * (1.0 - 0.9 * a.x * (1.0 - n * n));
            }
            p = mirror_repeat(q / sc + c);
        }
        if kind == 11u {
            let m = p * vec2f(a.x, a.y);
            let cell = floor(m);
            let f = m - cell;
            let odd = (cell - 2.0 * floor(cell / 2.0)) > vec2f(0.5);
            if a.z > 0.5 {
                p = select(f, 1.0 - f, odd);
            } else {
                p = f;
            }
        }
        if kind == 12u && a.x > 0.0 {
            let cell = vec2f(a.x / asp, a.x);
            p = min((floor(p / cell) + 0.5) * cell, vec2f(1.0));
        }
        if kind == 13u {
            let t = f32(index % 4096u) + h.z;
            let q = p * sc * a.y;
            let dx = value_noise(q + vec2f(t, 0.37 * t), seed) - 0.5;
            let dy = value_noise(q + vec2f(17.3, 11.1 + 0.83 * t), seed) - 0.5;
            p = mirror_repeat(p + vec2f(dx, dy) * 2.0 * a.x);
        }
        if kind == 14u {
            let block = bitcast<u32>(i32(floor(p.y / max(a.y, 0.001))));
            let hh = hash3(seed, index, block);
            if unit(hh) < a.z {
                let offset = (unit(lowbias(hh ^ 0x68bc21ebu)) * 2.0 - 1.0) * a.x;
                p.x = fract(p.x + offset);
                if unit(lowbias(hh ^ 0x02e5be93u)) < 0.35 {
                    split.x += offset * 0.3;
                }
            }
        }
        if kind == 15u {
            p = mirror_repeat((p - 0.5) / a.z + 0.5 - a.xy);
        }
        if kind == 16u {
            let row = bitcast<u32>(i32(floor(p.y * size.y)));
            let jitter = (unit(hash3(seed, index, row)) - 0.5) * a.x * 0.05;
            p = mirror_repeat(vec2f(p.x + jitter, fract(p.y + h.z)));
        }
    }
    if seen == 0.0 {
        return vec4f(0.0);
    }

    // How: the blurs share one pass of taps, the split pulls the red and blue apart, the edge
    // detect looks at what results.
    var edge = 0.0;
    var glow = false;
    var smears = false;
    for (var k = n_uv; k < n_uv + n_sample; k++) {
        let kind = u32(head(i, k).x);
        let a = part(i, k, 0u);
        if kind == 23u {
            split += a.x * vec2f(cos(a.y) / asp, sin(a.y));
        } else if kind == 24u {
            edge += a.x;
            glow = a.y > 0.5;
        } else {
            smears = true;
        }
    }
    var sum = vec4f(0.0);
    var count = 1.0;
    if smears {
        count = f32(TAPS);
        let reach = min(size.x, size.y) * 0.05 / size;
        for (var n = 0u; n < TAPS; n++) {
            let t = (f32(n) + 0.5) / f32(TAPS);
            var q = p;
            for (var k = n_uv; k < n_uv + n_sample; k++) {
                let kind = u32(head(i, k).x);
                let a = part(i, k, 0u);
                if kind == 20u {
                    let r = sqrt(t);
                    let angle = f32(n) * 2.399963;
                    q += vec2f(cos(angle), sin(angle)) * r * a.x * reach;
                } else if kind == 21u {
                    let d = (p - a.yz) * sc;
                    q += (rot(d, a.x * (t - 0.5)) - d) / sc;
                } else if kind == 22u {
                    q += (p - a.yz) * a.x * (t - 0.5);
                }
            }
            sum += read(q, split);
        }
    } else {
        sum = read(p, split);
    }
    sum /= count;
    var c = vec4f(0.0);
    if sum.a > 0.0 {
        c = vec4f(sum.rgb / sum.a, sum.a);
    }
    if edge > 0.0 {
        let o = 1.0 / size;
        let gx = grey(p + o * vec2f(1.0, -1.0)) + 2.0 * grey(p + o * vec2f(1.0, 0.0)) + grey(p + o * vec2f(1.0, 1.0))
            - grey(p + o * vec2f(-1.0, -1.0)) - 2.0 * grey(p + o * vec2f(-1.0, 0.0)) - grey(p + o * vec2f(-1.0, 1.0));
        let gy = grey(p + o * vec2f(-1.0, 1.0)) + 2.0 * grey(p + o * vec2f(0.0, 1.0)) + grey(p + o * vec2f(1.0, 1.0))
            - grey(p + o * vec2f(-1.0, -1.0)) - 2.0 * grey(p + o * vec2f(0.0, -1.0)) - grey(p + o * vec2f(1.0, -1.0));
        let e = clamp(length(vec2f(gx, gy)) * edge, 0.0, 1.0);
        if glow {
            c = vec4f(c.rgb + vec3f(e), c.a);
        } else {
            c = vec4f(vec3f(e), c.a);
        }
    }

    // What colour: the colour steps, first to last.
    var rgb = c.rgb;
    for (var k = n_uv + n_sample; k < n_uv + n_sample + n_colour; k++) {
        let h = head(i, k);
        let a = part(i, k, 0u);
        let b = part(i, k, 1u);
        let d = part(i, k, 2u);
        let kind = u32(h.x);
        if kind == 30u {
            rgb = vec3f(dot(a.xyz, rgb) + a.w, dot(b.xyz, rgb) + b.w, dot(d.xyz, rgb) + d.w);
        } else if kind == 31u {
            let levels = max(a.x - 1.0, 1.0);
            rgb = decode(clamp(floor(encode(rgb) * levels + 0.5) / levels, vec3f(0.0), vec3f(1.0)));
        } else if kind == 32u {
            let l = pow(max(dot(LUMA, rgb), 0.0), 1.0 / 2.2);
            var t = select(0.0, 1.0, l >= a.x);
            if a.y > 0.0001 {
                t = smoothstep(a.x - a.y * 0.5, a.x + a.y * 0.5, l);
            }
            rgb = mix(b.xyz, d.xyz, t);
        } else if kind == 33u {
            let e = pow(max(dot(LUMA, rgb), 0.0), 1.0 / 2.2);
            rgb = mix(rgb, mix(b.xyz, d.xyz, e), a.x);
        } else if kind == 34u {
            let e = encode(clamp(rgb, vec3f(0.0), vec3f(1.0)));
            rgb = decode(select(e, 1.0 - e, e > vec3f(a.x)));
        } else if kind == 35u {
            let range = max(a.y - a.x, 0.001);
            let e = clamp((encode(clamp(rgb, vec3f(0.0), vec3f(1.0))) - a.x) / range, vec3f(0.0), vec3f(1.0));
            rgb = decode(pow(e, vec3f(1.0 / max(a.z, 0.01))));
        } else if kind == 36u {
            let uv = (at - 0.5) * 2.0;
            let square = max(abs(uv.x), abs(uv.y));
            let round = length(uv) / 1.4142135;
            let dist = mix(square, round, a.z);
            let t = clamp((dist - (1.0 - a.y)) / max(a.y, 0.01), 0.0, 1.0);
            rgb = rgb * (1.0 - a.x * t * t * (3.0 - 2.0 * t));
        } else if kind == 37u {
            let cell = 1.0 + a.y * 15.0;
            let g = floor(at * size / cell);
            let seed = u32(h.y) ^ (u32(a.z) * 0x9e3779b1u);
            let n = (unit(hash3(seed, bitcast<u32>(i32(g.x)), bitcast<u32>(i32(g.y)))) - 0.5) * 2.0 * a.x * 0.5;
            rgb = decode(clamp(encode(clamp(rgb, vec3f(0.0), vec3f(1.0))) + vec3f(n), vec3f(0.0), vec3f(1.0)));
        } else if kind == 38u {
            let y = at.y * a.x - a.z;
            rgb = rgb * (1.0 - a.y * (0.5 - 0.5 * cos(TAU * y)));
        }
        rgb = clamp(rgb, vec3f(0.0), vec3f(1.0));
    }
    return vec4f(clamp(rgb, vec3f(0.0), vec3f(1.0)), c.a);
}

// The picture's luminance at `at`, for the edge detect.
fn grey(at: vec2f) -> f32 {
    let t = tap(at);
    return dot(LUMA, t.rgb * t.a);
}

// The picture's light times its cover at `at`, its red and blue taken from either side of it by
// `split` when that is not nothing.
fn read(at: vec2f, split: vec2f) -> vec4f {
    let g = tap(at);
    if all(split == vec2f(0.0)) {
        return vec4f(g.rgb * g.a, g.a);
    }
    let r = tap(at + split);
    let b = tap(at - split);
    return vec4f(r.r * r.a, g.g * g.a, b.b * b.a, (r.a + g.a + b.a) / 3.0);
}

fn mask_point(i: u32) -> vec2f {
    let pair = masks.points[i / 2u];
    if (i % 2u) == 0u {
        return pair.xy;
    }
    return pair.zw;
}

// Even-odd rule: a ray from `p` crosses the outline an odd number of times when `p` is inside.
fn inside(m: u32, p: vec2f) -> bool {
    let first = masks.info[m].x;
    let count = masks.info[m].y;
    var odd = false;
    var j = count - 1u;
    for (var i = 0u; i < count; i++) {
        let a = mask_point(first + i);
        let b = mask_point(first + j);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            odd = !odd;
        }
        j = i;
    }
    return odd;
}

// 1 inside, falling smoothly to 0 at the edge, over `width` (0: no fade).
fn ramp(t: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return 1.0;
    }
    let s = clamp(t / width, 0.0, 1.0);
    return s * s * (3.0 - 2.0 * s);
}

// Dividing by the weight here, after the interpolation, keeps straight lines of the picture
// straight across a quad seen in perspective.
@fragment
fn fs(in: Out) -> @location(0) vec4f {
    // The slice's texture coordinate is in the composition; the picture sits at `place` in it.
    let at = (in.uvq.xy / in.uvq.z - in.place.xy) / in.place.zw;
    let colour = shade(u32(in.fx), at);
    let within = all(at >= vec2f(0.0)) && all(at <= vec2f(1.0));
    let l = in.local / in.uvq.z;
    let edge = ramp(l.x, in.soft.x) * ramp(1.0 - l.x, in.soft.y) * ramp(l.y, in.soft.z)
        * ramp(1.0 - l.y, in.soft.w);
    var shown = 0.0;
    if edge > 0.0 && within {
        shown = pow(edge, 1.0 / max(in.gamma, 0.01));
    }
    let p = in.pos.xy / masks.size.xy;
    let count = u32(masks.size.z);
    for (var m = 0u; m < count; m++) {
        if inside(m, p) != (masks.info[m].z != 0u) {
            shown = 0.0;
        }
    }
    // Outside its place the picture leaves what is below; inside, it covers it by its opacity,
    // and a soft edge or a mask darkens both alike.
    var cover = 0.0;
    if within {
        cover = colour.a * in.opacity;
    }
    let rgb = colour.rgb * shown;
    if in.shading < 0.5 {
        return vec4f(rgb, cover);
    }
    if in.shading < 1.5 {
        return vec4f(rgb * cover, 1.0);
    }
    return vec4f(mix(vec3f(1.0), rgb, cover), 1.0);
}

