/// A seed from an effect's id, small enough for an f32 to hold exactly.
pub fn seed_of(id: &str) -> u32 {
    // FNV-1a.
    let hash = id.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    hash & 0x00ff_ffff
}

/// A well mixed number from `x` (the "lowbias32" hash).
pub fn lowbias(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// A well mixed number from three: the seed of an effect, a step of time, a pixel or a row.
pub fn hash3(a: u32, b: u32, c: u32) -> u32 {
    lowbias(a ^ lowbias(b ^ lowbias(c)))
}

/// 0 up to but not including 1 from a hash.
pub fn unit(hash: u32) -> f32 {
    (hash >> 8) as f32 / 16_777_216.0
}

/// Smooth noise of `p`, 0 to 1: the hashes of the corners of its cell, blended.
pub fn value_noise(p: [f32; 2], seed: u32) -> f32 {
    let (ix, iy) = (p[0].floor(), p[1].floor());
    let (fx, fy) = (p[0] - ix, p[1] - iy);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let corner = |x: f32, y: f32| unit(hash3(seed, x as i32 as u32, y as i32 as u32));
    let top = mix(corner(ix, iy), corner(ix + 1.0, iy), ux);
    let bottom = mix(corner(ix, iy + 1.0), corner(ix + 1.0, iy + 1.0), ux);
    mix(top, bottom, uy)
}

pub(super) fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
