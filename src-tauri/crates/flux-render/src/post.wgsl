// The passes of the composition's effects (see `post.rs`, which the CPU monitor mirrors): each draws
// one triangle over the whole target, reading up to two images of the composition's size or of a
// level of a bloom's chain. Light is linear and kept within 0 to 1.

struct Params {
    a: vec4f,
    b: vec4f,
    c: vec4f,
    d: vec4f,
};

@group(0) @binding(0) var first: texture_2d<f32>;
@group(0) @binding(1) var second: texture_2d<f32>;
@group(0) @binding(2) var linear_sampler: sampler;
@group(0) @binding(3) var<uniform> p: Params;

struct Out {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    // One triangle that covers the target: (0, 0), (2, 0) and (0, 2) in picture coordinates.
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.pos = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

const LUMA: vec3f = vec3f(0.2126, 0.7152, 0.0722);

fn read(uv: vec2f) -> vec3f {
    return textureSampleLevel(first, linear_sampler, uv, 0.0).rgb;
}

fn read_second(uv: vec2f) -> vec3f {
    return textureSampleLevel(second, linear_sampler, uv, 0.0).rgb;
}

fn hue_turn(c: vec3f, angle: f32) -> vec3f {
    let s = sin(angle);
    let co = cos(angle);
    let k = (1.0 - co) / 3.0;
    let d = s / sqrt(3.0);
    return vec3f(
        (co + k) * c.r + (k - d) * c.g + (k + d) * c.b,
        (k + d) * c.r + (co + k) * c.g + (k - d) * c.b,
        (k - d) * c.r + (k + d) * c.g + (co + k) * c.b,
    );
}

// Where the old frame is read for the pixel at `uv`: drawn `scale` times as large, turned and
// moved about the centre of the frame (`aspect` is its width over its height).
fn history_at(uv: vec2f, scale: f32, angle: f32, shift: vec2f, aspect: f32) -> vec2f {
    let q = vec2f((uv.x - 0.5) * aspect / scale, (uv.y - 0.5) / scale);
    let s = sin(-angle);
    let c = cos(-angle);
    let r = vec2f(c * q.x - s * q.y, s * q.x + c * q.y);
    return vec2f(r.x / aspect + 0.5 - shift.x, r.y + 0.5 - shift.y);
}

fn old_frame(uv: vec2f, turn_hue: bool) -> vec3f {
    let at = history_at(uv, p.a.y, p.a.z, p.b.xy, p.b.w);
    var old = vec3f(0.0);
    if all(at >= vec2f(0.0)) && all(at <= vec2f(1.0)) {
        old = read_second(at);
    }
    if turn_hue {
        old = hue_turn(old, p.a.w);
    }
    return old;
}

// `first` is the new frame and `second` the old one. a: keep, scale, angle, hue; b: shift, mode, aspect.
@fragment
fn fs_feedback(in: Out) -> @location(0) vec4f {
    let cur = read(in.uv);
    let old = old_frame(in.uv, true);
    let keep = p.a.x;
    var v = cur;
    if p.b.z < 0.5 {
        v = cur + (old - cur) * keep;
    } else if p.b.z < 1.5 {
        v = cur + old * keep;
    } else {
        v = max(cur, old * keep);
    }
    return vec4f(clamp(v, vec3f(0.0), vec3f(1.0)), 1.0);
}

// a: keep; b: mode.
@fragment
fn fs_trails(in: Out) -> @location(0) vec4f {
    let cur = read(in.uv);
    let old = read_second(in.uv);
    let keep = p.a.x;
    var v = cur;
    if p.b.z < 0.5 {
        v = cur + (old - cur) * keep;
    } else if p.b.z < 1.5 {
        v = max(cur, old * keep);
    } else {
        v = cur + old * keep;
    }
    return vec4f(clamp(v, vec3f(0.0), vec3f(1.0)), 1.0);
}

// The weight of the bright pass: 0 below the threshold, to 1 at white.
fn bright(c: vec3f, threshold: f32) -> f32 {
    let seen = pow(max(dot(LUMA, c), 0.0), 1.0 / 2.2);
    return clamp((seen - threshold) / max(1.0 - threshold, 0.05), 0.0, 1.0);
}

// Four taps a source pixel away from the pixel, `c.xy` being the source's pixel in picture units.
fn four(uv: vec2f, texel: vec2f, cut: bool) -> vec3f {
    var sum = vec3f(0.0);
    for (var n = 0u; n < 4u; n++) {
        let d = vec2f(select(-1.0, 1.0, (n & 1u) == 1u), select(-1.0, 1.0, n >= 2u));
        var c = read(uv + d * texel);
        if cut {
            c = c * bright(c, p.a.x);
        }
        sum += c * 0.25;
    }
    return sum;
}

// a: threshold; c: the source's pixel.
@fragment
fn fs_bright(in: Out) -> @location(0) vec4f {
    return vec4f(four(in.uv, p.c.xy, true), 1.0);
}

@fragment
fn fs_down(in: Out) -> @location(0) vec4f {
    return vec4f(four(in.uv, p.c.xy, false), 1.0);
}

// Added into what the target holds: a: gain; c: half a pixel of the smaller image.
@fragment
fn fs_up(in: Out) -> @location(0) vec4f {
    return vec4f(four(in.uv, p.c.xy, false) * p.a.x, 0.0);
}

// The picture and its glow, a: intensity.
@fragment
fn fs_mix(in: Out) -> @location(0) vec4f {
    let v = read(in.uv) + read_second(in.uv) * p.a.x;
    return vec4f(clamp(v, vec3f(0.0), vec3f(1.0)), 1.0);
}
