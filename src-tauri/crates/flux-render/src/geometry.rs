use super::*;

/// Where a picture of `picture` pixels goes in a window of `window` pixels: as large as it fits
/// with its shape kept, centred. `(x, y, width, height)`.
pub fn fit(picture: (u32, u32), window: (u32, u32)) -> (f32, f32, f32, f32) {
    let (pw, ph) = (picture.0.max(1) as f32, picture.1.max(1) as f32);
    let (ww, wh) = (window.0 as f32, window.1 as f32);
    let scale = (ww / pw).min(wh / ph);
    let (w, h) = (pw * scale, ph * scale);
    ((ww - w) / 2.0, (wh - h) / 2.0, w, h)
}

/// The quad that shows the whole picture fitted to the window, as [`fit`] places it.
pub fn fitted_quad(picture: (u32, u32), window: (u32, u32)) -> Quad {
    let (x, y, w, h) = fit(picture, window);
    let (ww, wh) = (window.0.max(1) as f32, window.1.max(1) as f32);
    let (left, top, right, bottom) = (x / ww, y / wh, (x + w) / ww, (y + h) / wh);
    Quad {
        input: [0.0, 0.0, 1.0, 1.0],
        corners: [[left, top], [right, top], [right, bottom], [left, bottom]],
        soft: [0.0; 4],
        gamma: 1.0,
    }
}

/// The two triangles of `quad`. Each corner's texture coordinate is weighted by how far it is from
/// where the quad's diagonals cross, relative to the opposite corner; the fragment shader divides
/// the weight out again, which maps the rectangle onto the four corners in perspective instead of
/// folding it along the diagonal the triangles share.
pub fn vertices(quad: &Quad) -> [Vertex; 6] {
    let [x, y, w, h] = quad.input;
    let uv = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    let local = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let p = quad
        .corners
        .map(|[cx, cy]| [cx * 2.0 - 1.0, 1.0 - cy * 2.0]);
    let weights = diagonal_weights(&p);
    let corner = |i: usize| Vertex {
        pos: p[i],
        uvq: [uv[i][0] * weights[i], uv[i][1] * weights[i], weights[i]],
        local: [local[i][0] * weights[i], local[i][1] * weights[i]],
        soft: quad.soft.map(|v| v.clamp(0.0, 1.0)),
        gamma: quad.gamma,
        place: [0.0, 0.0, 1.0, 1.0],
        opacity: 1.0,
        shading: 0.0,
        fx: 0.0,
    };
    [
        corner(0),
        corner(1),
        corner(2),
        corner(0),
        corner(2),
        corner(3),
    ]
}

/// For corners `p` in order around the quad: `(d[i] + d[i+2]) / d[i+2]`, with `d` each corner's
/// distance to where the diagonals cross. All ones when the diagonals do not cross inside (a
/// folded or degenerate quad), which draws it without perspective rather than not at all.
pub(super) fn diagonal_weights(p: &[[f32; 2]; 4]) -> [f32; 4] {
    let sub = |a: [f32; 2], b: [f32; 2]| [a[0] - b[0], a[1] - b[1]];
    let cross = |a: [f32; 2], b: [f32; 2]| a[0] * b[1] - a[1] * b[0];
    let (r, s) = (sub(p[2], p[0]), sub(p[3], p[1]));
    let denominator = cross(r, s);
    if denominator.abs() < 1e-9 {
        return [1.0; 4];
    }
    let q0 = sub(p[1], p[0]);
    let t = cross(q0, s) / denominator;
    let u = cross(q0, r) / denominator;
    if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
        return [1.0; 4];
    }
    let center = [p[0][0] + t * r[0], p[0][1] + t * r[1]];
    let d = p.map(|corner| {
        let [dx, dy] = sub(corner, center);
        (dx * dx + dy * dy).sqrt()
    });
    std::array::from_fn(|i| {
        let opposite = d[(i + 2) % 4];
        if opposite < 1e-9 {
            1.0
        } else {
            (d[i] + opposite) / opposite
        }
    })
}

pub(super) fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8> {
    vertices
        .iter()
        .flat_map(|v| {
            v.pos
                .iter()
                .chain(&v.uvq)
                .chain(&v.local)
                .chain(&v.soft)
                .chain([&v.gamma])
                .chain(&v.place)
                .chain([&v.opacity, &v.shading, &v.fx])
                .flat_map(|f| f.to_ne_bytes())
        })
        .collect()
}

pub(super) fn effect_bytes(effects: &[[f32; effects::FX_FLOATS]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(effects.len() * effects::FX_FLOATS * 4);
    bytes.extend(effects.iter().flatten().flat_map(|f| f.to_ne_bytes()));
    bytes
}

/// The masks' uniform for a window of `window` pixels: at most [`MAX_MASKS`] masks of 3 to
/// [`MAX_MASK_POINTS`] points; others are left out.
pub(super) fn mask_bytes(masks: &[Mask], window: (u32, u32)) -> Vec<u8> {
    let shown: Vec<&Mask> = masks
        .iter()
        .filter(|m| (3..=MAX_MASK_POINTS).contains(&m.points.len()))
        .take(MAX_MASKS)
        .collect();
    let mut bytes = Vec::with_capacity(MASKS_SIZE as usize);
    for f in [window.0 as f32, window.1 as f32, shown.len() as f32, 0.0] {
        bytes.extend_from_slice(&f.to_ne_bytes());
    }
    let mut first = 0u32;
    for index in 0..MAX_MASKS {
        let (count, invert) = shown
            .get(index)
            .map_or((0, false), |m| (m.points.len() as u32, m.invert));
        for u in [first, count, u32::from(invert), 0] {
            bytes.extend_from_slice(&u.to_ne_bytes());
        }
        first += count;
    }
    let points = shown.iter().flat_map(|m| m.points.iter().flatten());
    for f in points
        .copied()
        .chain(std::iter::repeat(0.0))
        .take(2 * MAX_MASKS * MAX_MASK_POINTS)
    {
        bytes.extend_from_slice(&f.to_ne_bytes());
    }
    bytes
}
