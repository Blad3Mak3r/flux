use super::*;

/// A picture to show: `rgba` is `width * height` pixels of 8-bit sRGB RGBA, rows top to bottom.
/// `id` tells pictures apart, so the same one is not uploaded again.
#[derive(Debug, Clone, Copy)]
pub struct Picture<'a> {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
    /// Where it sits in the composition, `[x, y, width, height]` as fractions of it (see
    /// [`placement`]); outside it, what is below shows.
    pub place: [f32; 4],
    /// How much of it covers what is below, 0 to 1.
    pub opacity: f32,
    /// How it mixes with what is below, at its opacity.
    pub blend: Blend,
    /// What is done to its colours and shape before it is drawn.
    pub effects: Effects,
    /// Read texel by texel with no smoothing, so a one pixel line stays one pixel: a test card's.
    /// The window is then drawn without smoothing too, wherever any picture asks for it.
    pub nearest: bool,
}

/// A group of pictures, as Resolume's layer groups: the pictures are mixed together over a clear
/// picture first, and that mix is drawn like a picture of its own, with the group's effects, blend
/// and opacity.
#[derive(Debug, Clone)]
pub struct Group<'a> {
    /// The pictures, the first at the bottom.
    pub pictures: Vec<Picture<'a>>,
    /// How much the mix covers what is below, 0 to 1.
    pub opacity: f32,
    pub blend: Blend,
    /// What is done to the mix before it is drawn.
    pub effects: Effects,
}

/// What is drawn in the composition: a picture, or a group of them.
#[derive(Debug, Clone)]
pub enum Item<'a> {
    Picture(Picture<'a>),
    Group(Group<'a>),
}

impl Item<'_> {
    /// Whether a picture of it is read texel by texel (see [`Picture::nearest`]).
    pub(super) fn nearest(&self) -> bool {
        match self {
            Self::Picture(picture) => picture.nearest,
            Self::Group(group) => group.pictures.iter().any(|picture| picture.nearest),
        }
    }

    /// How many slots of the effects it takes: a picture, or its pictures and the group.
    pub(super) fn count(&self) -> usize {
        match self {
            Self::Picture(_) => 1,
            Self::Group(group) => group.pictures.len() + 1,
        }
    }
}

/// How a picture mixes with what is drawn below it, as Resolume's layer blends do: the picture
/// (its own alpha times its opacity) is composited over the layers below, where it is clear what
/// is below shows, and `mix(below, blend(below, picture), alpha * opacity)` is what results.
/// Resolume blends the encoded (gamma) values, not the light, so the formulas and the mix work on
/// sRGB-encoded values, every blend alike.
///
/// Every blend needs the light below as the picture is drawn, which the shader reads from a copy
/// of it: the pictures are drawn into the textures of the composition (see [`compose`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Blend {
    /// Covers what is below.
    #[default]
    Alpha,
    /// Adds its light to what is below.
    Add,
    /// Takes its light from what is below.
    Subtract,
    /// Darkens what is below by its colour.
    Multiply,
    /// Lightens what is below, softer than adding.
    Screen,
    /// Keeps the lighter of the two.
    Lighten,
    /// Keeps the darker of the two.
    Darken,
    /// The distance between the two lights.
    Difference,
    /// Like difference, with less contrast.
    Exclusion,
    /// Multiplies where what is below is dark, screens where it is light.
    Overlay,
    /// A gentle contrast driven by the picture.
    SoftLight,
    /// Overlay driven by the picture's light.
    HardLight,
    /// Brightens what is below by the picture.
    ColorDodge,
    /// Darkens what is below by the picture, harder than multiply.
    ColorBurn,
    /// Burns or dodges linearly.
    LinearLight,
    /// Burns or dodges, harder than linear light.
    VividLight,
    /// Keeps the darker or the lighter of the two by the picture's light.
    PinLight,
    /// The picture's hue on the saturation and luminosity below.
    Hue,
    /// The picture's saturation on the hue and luminosity below.
    Saturation,
    /// The picture's hue and saturation on the luminosity below.
    Color,
    /// The picture's luminosity on the hue and saturation below.
    Luminosity,
    /// A hard cut: where its cover is at least half it replaces what is below, elsewhere it leaves it (Resolume's AlphaCut).
    AlphaCut,
    /// Its dark parts turn transparent: its alpha is multiplied by its luma (Resolume's Luma Key).
    LumaKey,
    /// Its bright parts turn transparent: its alpha is multiplied by one minus its luma (Resolume's Luma Key I).
    LumaKeyInverted,
    /// Its luma replaces its own alpha: bright is opaque, dark is transparent, wherever it has a picture.
    LumaIsAlpha,
    /// The inverse of the distance between the two lights, a lighter picture (Resolume's Difference I).
    DifferenceInverted,
    /// Acts as a mask: what is below shows where it is bright and goes dark where it is dark; it is not drawn (Resolume's 50Mask).
    Mask50,
    /// Takes over what is below one channel at a time as its cover rises: blue, then green, then red.
    Rgb,
    /// Shifts what is below sideways by its luma, none at mid grey; it is not drawn.
    Displace,
}

impl Blend {
    /// Every blend, in the order of the shader's numbers (see [`Self::shading`]).
    pub const ALL: [Self; 29] = [
        Self::Alpha,
        Self::Add,
        Self::Subtract,
        Self::Multiply,
        Self::Screen,
        Self::Lighten,
        Self::Darken,
        Self::Difference,
        Self::Exclusion,
        Self::Overlay,
        Self::SoftLight,
        Self::HardLight,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::LinearLight,
        Self::VividLight,
        Self::PinLight,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
        Self::AlphaCut,
        Self::LumaKey,
        Self::LumaKeyInverted,
        Self::LumaIsAlpha,
        Self::DifferenceInverted,
        Self::Mask50,
        Self::Rgb,
        Self::Displace,
    ];

    /// What the shader gets for it: the blend's place in [`Self::ALL`].
    pub(super) fn shading(self) -> f32 {
        Self::ALL.iter().position(|b| *b == self).unwrap_or(0) as f32
    }
}

/// How a picture is sized into the composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sizing {
    /// As large as it fits with its shape kept, centred.
    Fit,
    /// As small as it covers the composition with its shape kept, centred.
    Fill,
    /// Exactly the composition.
    Stretch,
    /// At its own size in pixels, centred.
    None,
}

/// Where a picture of `picture` pixels sits in a composition of `composition` pixels when sized
/// `sizing`: `[x, y, width, height]` as fractions of the composition, which can reach past it.
pub fn placement(sizing: Sizing, picture: (u32, u32), composition: (u32, u32)) -> [f32; 4] {
    let (pw, ph) = (picture.0.max(1) as f32, picture.1.max(1) as f32);
    let (cw, ch) = (composition.0.max(1) as f32, composition.1.max(1) as f32);
    let scale = match sizing {
        Sizing::Stretch => return [0.0, 0.0, 1.0, 1.0],
        Sizing::Fit => (cw / pw).min(ch / ph),
        Sizing::Fill => (cw / pw).max(ch / ph),
        Sizing::None => 1.0,
    };
    let (w, h) = (pw * scale / cw, ph * scale / ch);
    [(1.0 - w) / 2.0, (1.0 - h) / 2.0, w, h]
}

/// `place` (see [`placement`]) drawn `scale` times as large about its centre, and that centre
/// moved by `position`, as fractions of the composition's width and height.
pub fn transformed(place: [f32; 4], scale: f32, position: [f32; 2]) -> [f32; 4] {
    let [x, y, w, h] = place;
    let (cx, cy) = (x + w / 2.0 + position[0], y + h / 2.0 + position[1]);
    let (w, h) = (w * scale, h * scale);
    [cx - w / 2.0, cy - h / 2.0, w, h]
}
