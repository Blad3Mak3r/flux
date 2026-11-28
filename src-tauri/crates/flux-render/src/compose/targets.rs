use super::*;

/// The format the composition is drawn in: half floats, which keep the light of a frame in
/// linear light without banding, when the adapter can draw on them, filter them and blend into
/// them; else 8-bit sRGB, which the GPU converts as it reads and writes.
pub(crate) fn composition_format(
    usages: wgpu::TextureUsages,
    flags: wgpu::TextureFormatFeatureFlags,
) -> wgpu::TextureFormat {
    use wgpu::{TextureFormatFeatureFlags as F, TextureUsages as U};
    let needs_usages = U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC | U::COPY_DST;
    if usages.contains(needs_usages) && flags.contains(F::FILTERABLE | F::BLENDABLE) {
        wgpu::TextureFormat::Rgba16Float
    } else {
        wgpu::TextureFormat::Rgba8UnormSrgb
    }
}

/// The size the composition is drawn at: its own, within what the GPU's textures hold.
pub(crate) fn work_size(composition: (u32, u32), limit: u32) -> (u32, u32) {
    (
        composition.0.clamp(1, limit.max(1)),
        composition.1.clamp(1, limit.max(1)),
    )
}

/// The size of a level of a bloom's chain: half of the one before, from the first at half of the work.
pub(super) fn level_size(work: (u32, u32), level: usize) -> (u32, u32) {
    (
        (work.0 >> (level + 1)).max(1),
        (work.1 >> (level + 1)).max(1),
    )
}

pub(super) struct Image {
    pub(super) texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
}

pub(super) fn image(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    usage: wgpu::TextureUsages,
    label: &str,
) -> Image {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    Image { texture, view }
}

/// The textures and what reads them, made for one size.
pub(super) struct Targets {
    pub(super) size: (u32, u32),
    /// The two textures the passes draw from one to the other.
    pub(super) work: [Image; 2],
    pub(super) history: Image,
    /// The levels of a bloom's chain.
    pub(super) levels: Vec<Image>,
    /// A work texture as a picture (for the picture shader).
    pub(super) as_picture: [wgpu::BindGroup; 2],
    /// The same, sampled without smoothing, for a window that shows a test card.
    pub(super) as_picture_crisp: [wgpu::BindGroup; 2],
    /// The pair of textures a group's pictures are mixed in, and each as a picture.
    pub(super) inner: [Image; 2],
    pub(super) inner_picture: [wgpu::BindGroup; 2],
    /// A work texture and the history, for the passes that combine them.
    pub(super) with_history: [wgpu::BindGroup; 2],
    /// A work texture and the first level of a bloom's chain.
    pub(super) with_bloom: [wgpu::BindGroup; 2],
    /// A work texture on its own, and each level of the chain.
    pub(super) work_alone: [wgpu::BindGroup; 2],
    pub(super) level_alone: Vec<wgpu::BindGroup>,
}

/// What a draw of a frame with effects needs from the screen.
pub(crate) struct Inputs<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub vertices: &'a wgpu::Buffer,
    /// The pictures and groups, the bottom one first.
    pub layers: &'a [Drawn<'a>],
    /// The vertices of the program's picture.
    pub program: Range<u32>,
    pub post: &'a Post,
    pub composition: (u32, u32),
}

/// What one step of the first pass draws: a picture (its texture's group and the vertices that
/// draw it), or a group of pictures (the same for each) with the vertices that draw their mix.
pub(crate) enum Drawn<'a> {
    Picture(&'a wgpu::BindGroup, Range<u32>),
    Group(Vec<(&'a wgpu::BindGroup, Range<u32>)>, Range<u32>),
}

/// The textures a picture is drawn between: it is drawn into `to` over a copy of `from`, which the
/// shader reads (as `backdrop`) for the light below.
pub(super) struct Pair<'a> {
    pub(super) from: &'a Image,
    pub(super) to: &'a Image,
    pub(super) backdrop: &'a wgpu::BindGroup,
}

pub(crate) struct Compose {
    pub(super) format: wgpu::TextureFormat,
    pub(super) post_layout: wgpu::BindGroupLayout,
    pub(super) feedback: wgpu::RenderPipeline,
    pub(super) trails: wgpu::RenderPipeline,
    pub(super) bright: wgpu::RenderPipeline,
    pub(super) down: wgpu::RenderPipeline,
    pub(super) up: wgpu::RenderPipeline,
    pub(super) mix: wgpu::RenderPipeline,
    /// The picture shader's pipeline drawing into the work textures with the light below, and the
    /// one drawing a picture plainly over a cleared one.
    pub(super) picture: wgpu::RenderPipeline,
    pub(super) plain: wgpu::RenderPipeline,
    pub(super) params: wgpu::Buffer,
    /// Reads a texel as it is, with no blend between texels.
    pub(super) nearest: wgpu::Sampler,
    /// The groups of the picture shader's second binding group: the pictures' own effects with no
    /// masks, the program's with no masks, and the masks of the window with no effects.
    pub(crate) pictures_group: wgpu::BindGroup,
    pub(super) program_group: wgpu::BindGroup,
    pub(crate) window_group: wgpu::BindGroup,
    pub(super) program_effects: wgpu::Buffer,
    pub(super) targets: Option<Targets>,
    /// Whether the history holds the frame before.
    pub(super) history_valid: bool,
    /// The texture of the work pair the last draw ended in.
    pub(super) current: usize,
}

/// What the groups of the picture shader's second binding are made of.
pub(crate) struct Shared<'a> {
    pub masks_layout: &'a wgpu::BindGroupLayout,
    pub masks: &'a wgpu::Buffer,
    pub effects: &'a wgpu::Buffer,
}
