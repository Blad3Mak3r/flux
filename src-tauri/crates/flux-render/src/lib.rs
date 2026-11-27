//! Draws video frames on an output window through the GPU (wgpu). The picture reaches the window as
//! [`Quad`]s: each takes a rectangle of the picture and stretches it between four corners of the
//! window, with the perspective of a projector seen at an angle (a "corner pin"). With no quads the
//! whole picture is fitted to the window keeping its shape, with black around it; the window is
//! black when there is no picture. Several pictures are drawn one over another, each at its own
//! place and opacity, which is how clips fade and cross-fade, and mixed with what is below by its
//! [`Blend`]. A quad can fade out towards its edges (a soft edge, to blend
//! two projectors where their pictures overlap), and [`Mask`]s black out parts of the window.

mod capture;
mod compose;
mod composite;
pub mod effects;
pub mod mapped;
pub mod palette;
pub mod pattern;
pub mod post;

pub use capture::Captured;
pub use composite::{composite, composite_items, composite_post};
pub use effects::{Effects, Time};
pub use mapped::map_screen;
pub use post::{History, Post};

use thiserror::Error;
use wgpu::rwh::{HasDisplayHandle, HasWindowHandle};

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("the window cannot be drawn on: {0}")]
    Surface(#[from] wgpu::CreateSurfaceError),
    #[error("no graphics adapter can draw on the window: {0}")]
    Adapter(#[from] wgpu::RequestAdapterError),
    #[error("the graphics device could not start: {0}")]
    Device(#[from] wgpu::RequestDeviceError),
    #[error("the window's surface was lost")]
    Lost,
}

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
    fn nearest(&self) -> bool {
        match self {
            Self::Picture(picture) => picture.nearest,
            Self::Group(group) => group.pictures.iter().any(|picture| picture.nearest),
        }
    }

    /// How many slots of the effects it takes: a picture, or its pictures and the group.
    fn count(&self) -> usize {
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
    fn shading(self) -> f32 {
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

/// A rectangle of the picture shown between four points of the window: the slice of a mapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    /// `[x, y, width, height]` of the picture, each 0 to 1, from its top left corner.
    pub input: [f32; 4],
    /// Where the rectangle's top left, top right, bottom right and bottom left corners go, as
    /// `[x, y]` from 0 to 1 of the window's width and height, from its top left corner.
    pub corners: [[f32; 2]; 4],
    /// How far in from its left, right, top and bottom edges the quad fades to black, each a
    /// fraction (0 to 1) of its width or height; 0 is a hard edge.
    pub soft: [f32; 4],
    /// The fade's curve: 1 fades the light evenly; more keeps it brighter longer, less darker.
    pub gamma: f32,
}

/// Most quads drawn at once.
pub const MAX_QUADS: usize = 64;
/// Pictures drawn one over another at most; more are left out from the bottom.
pub const MAX_LAYERS: usize = 16;

/// A shape on the window, as `[x, y]` points from 0 to 1 of its width and height, that hides the
/// picture inside it, or outside it when `invert`.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub points: Vec<[f32; 2]>,
    pub invert: bool,
}

/// Most masks drawn at once, and most points of one.
pub const MAX_MASKS: usize = 8;
pub const MAX_MASK_POINTS: usize = 32;
/// The masks' uniform: a header, one line of 4 numbers per mask and two points per line.
const MASKS_SIZE: u64 = 16 + 16 * MAX_MASKS as u64 + 8 * (MAX_MASKS * MAX_MASK_POINTS) as u64;
/// The effects' uniform: [`effects::FX_FLOATS`] floats for each of [`MAX_LAYERS`] pictures and
/// groups.
const EFFECTS_SIZE: u64 = (effects::FX_FLOATS * MAX_LAYERS * 4) as u64;

/// A corner of a quad as the GPU reads it: its place in clip space and its texture coordinate,
/// multiplied by a weight that undoes the perspective (see [`vertices`]).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub uvq: [f32; 3],
    /// Where in the quad it is (0 to 1 across, 0 to 1 down), weighted like `uvq`.
    pub local: [f32; 2],
    pub soft: [f32; 4],
    pub gamma: f32,
    /// Where the picture sits in the composition (see [`Picture::place`]).
    pub place: [f32; 4],
    pub opacity: f32,
    /// [`Blend::shading`].
    pub shading: f32,
    /// The picture's place in the layers, which picks its effects.
    pub fx: f32,
}

const VERTEX_SIZE: u64 = size_of::<Vertex>() as u64;

struct Uploaded {
    id: u64,
    width: u32,
    height: u32,
    nearest: bool,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// What the first pass draws: a picture by the slot of its texture, or a group of them and the
/// vertices of their mix.
enum Step {
    Picture(usize, std::ops::Range<u32>),
    Group(Vec<(usize, std::ops::Range<u32>)>, std::ops::Range<u32>),
}

/// What [`Screen::stage`] leaves for the passes: the first pass's steps and the vertices of the
/// program's picture and of the composition on the target.
struct Staged {
    steps: Vec<Step>,
    program: std::ops::Range<u32>,
    result: std::ops::Range<u32>,
}

/// A window's GPU surface, made before the device that draws on it.
pub struct Target {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
}

/// A window the pictures are drawn on.
pub struct Screen {
    /// `None` for a screen that draws offscreen (see [`Screen::offscreen`]).
    surface: Option<wgpu::Surface<'static>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Draws the composition onto the window.
    present: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    masks: wgpu::Buffer,
    effects: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The same without smoothing, for the pictures that ask for it.
    nearest: wgpu::Sampler,
    /// The texture of each layer, from the bottom.
    pictures: Vec<Option<Uploaded>>,
    errors: Errors,
    shader: wgpu::ShaderModule,
    pipeline_layouts: [wgpu::PipelineLayout; 2],
    masks_layout: wgpu::BindGroupLayout,
    /// The format the composition is drawn in when it has effects (see [`compose`]).
    composition_format: wgpu::TextureFormat,
    /// What draws a composition with effects, made when the first one is drawn.
    compose: Option<compose::Compose>,
    /// What reads the composition back, made by the first [`Screen::capture`].
    capture: Option<capture::Capture>,
}

/// The last error the GPU reported while drawing, shared with whoever reports it. A GPU error never
/// panics: the picture it hit is lost and drawing goes on.
pub type Errors = std::sync::Arc<std::sync::Mutex<Option<String>>>;

const SHADER: &str = include_str!("shader.wgsl");

/// The layouts of the picture shader's pipelines: the picture and the masks with the effects, and
/// those with the light below as a third group, which the blends that read it have.
fn pipeline_layouts(
    device: &wgpu::Device,
    picture: &wgpu::BindGroupLayout,
    masks: &wgpu::BindGroupLayout,
) -> [wgpu::PipelineLayout; 2] {
    let make = |groups: &[Option<&wgpu::BindGroupLayout>]| {
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("picture"),
            bind_group_layouts: groups,
            immediate_size: 0,
        })
    };
    [
        make(&[Some(picture), Some(masks)]),
        make(&[Some(picture), Some(masks), Some(picture)]),
    ]
}

/// The pipeline that draws a picture into a target of `format`: with `reads`, blended with the
/// light below it (a third group, `fs_blend`); else plainly over the target (`fs`).
fn picture_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layouts: &[wgpu::PipelineLayout; 2],
    format: wgpu::TextureFormat,
    reads: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("picture"),
        layout: Some(&layouts[usize::from(reads)]),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_SIZE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x2,
                    1 => Float32x3,
                    2 => Float32x2,
                    3 => Float32x4,
                    4 => Float32,
                    5 => Float32x4,
                    6 => Float32,
                    7 => Float32,
                    8 => Float32
                ],
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(if reads { "fs_blend" } else { "fs" }),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: (!reads).then_some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The group of the picture shader's first binding: a picture's texture and its sampler.
fn picture_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("picture"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// The group of its second: the masks and the effects.
fn masks_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("masks"),
        entries: &[0, 1].map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }),
    })
}

/// The quad that draws a picture over the whole of a target of the composition's size.
fn whole_quad() -> Quad {
    Quad {
        input: [0.0, 0.0, 1.0, 1.0],
        corners: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        soft: [0.0; 4],
        gamma: 1.0,
    }
}

impl Screen {
    /// Makes the GPU surface of `window`. Call it on the thread the platform wants windows touched
    /// from (the main thread on macOS); [`Screen::new`] can then run on any thread.
    pub fn target<W>(window: W) -> Result<Target, RenderError>
    where
        W: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static,
    {
        // Without a display handle: Vulkan, DX12 and Metal take what they need from the window,
        // and the GL backend's handle check refuses some window systems' handles.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window)?;
        Ok(Target { instance, surface })
    }

    /// Starts the GPU on `target`, whose drawable area is `width` x `height` pixels. Blocks until
    /// the device is ready, which can take a moment.
    pub fn new(target: Target, width: u32, height: u32) -> Result<Self, RenderError> {
        let Target { instance, surface } = target;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        Self::start(&adapter, Some(surface), width, height)
    }

    /// Starts the GPU with no window, to draw the composition with [`Screen::capture`].
    pub fn offscreen() -> Result<Self, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))?;
        Self::start(&adapter, None, 1, 1)
    }

    fn start(
        adapter: &wgpu::Adapter,
        surface: Option<wgpu::Surface<'static>>,
        width: u32,
        height: u32,
    ) -> Result<Self, RenderError> {
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("flux-render"),
                ..Default::default()
            }))?;
        let features = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Float);
        let composition_format =
            compose::composition_format(features.allowed_usages, features.flags);
        let errors = Errors::default();
        let sink = std::sync::Arc::clone(&errors);
        device.on_uncaptured_error(std::sync::Arc::new(move |error: wgpu::Error| {
            *sink
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
        }));

        let config = match &surface {
            Some(surface) => {
                let caps = surface.get_capabilities(adapter);
                // The pictures are sRGB, so an sRGB surface keeps their colours as they are.
                let format = caps
                    .formats
                    .iter()
                    .copied()
                    .find(wgpu::TextureFormat::is_srgb)
                    .unwrap_or(caps.formats[0]);
                let config = wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format,
                    width: width.max(1),
                    height: height.max(1),
                    // Waits for the display: one picture per refresh, never torn.
                    present_mode: wgpu::PresentMode::Fifo,
                    desired_maximum_frame_latency: 1,
                    alpha_mode: caps.alpha_modes[0],
                    view_formats: vec![],
                    color_space: wgpu::SurfaceColorSpace::Auto,
                };
                surface.configure(&device, &config);
                config
            }
            None => wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: capture::FORMAT,
                width: 1,
                height: 1,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 1,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                color_space: wgpu::SurfaceColorSpace::Auto,
            },
        };
        let format = config.format;

        let layout = picture_group_layout(&device);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("picture"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let masks_layout = masks_group_layout(&device);
        let masks = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("masks"),
            size: MASKS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let effects = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("effects"),
            size: EFFECTS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipeline_layouts = pipeline_layouts(&device, &layout, &masks_layout);
        let present = picture_pipeline(&device, &shader, &pipeline_layouts, format, false);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("picture"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("picture crisp"),
            ..Default::default()
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quads"),
            size: VERTEX_SIZE * 6 * (MAX_QUADS * MAX_LAYERS) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            surface,
            device,
            queue,
            config,
            present,
            vertices,
            masks,
            effects,
            layout,
            sampler,
            nearest,
            pictures: (0..MAX_LAYERS).map(|_| None).collect(),
            errors,
            shader,
            pipeline_layouts,
            masks_layout,
            composition_format,
            compose: None,
            capture: None,
        })
    }

    /// Where the last GPU error is kept.
    pub fn errors(&self) -> Errors {
        std::sync::Arc::clone(&self.errors)
    }

    /// The window's drawable area changed size.
    pub fn resize(&mut self, width: u32, height: u32) {
        if (width, height) != (self.config.width, self.config.height) && width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            if let Some(surface) = &self.surface {
                surface.configure(&self.device, &self.config);
            }
        }
    }

    /// Draws `items` one over another from the first (at most [`MAX_LAYERS`] pictures and groups
    /// together, the top ones kept), each placed in a composition of `composition` pixels,
    /// through `quads` (at most [`MAX_QUADS`]) or with the composition fitted to the window when
    /// there are none, with `masks` hiding parts of it; black for no picture. Waits for the
    /// display's next refresh, so a loop calling it runs at the display's rate.
    ///
    /// The pictures are drawn into a texture of the composition's size first, which the effects
    /// change (and remember, for the next frame) and which is then drawn onto the window through
    /// the quads and masks as if it were a picture.
    pub fn draw(
        &mut self,
        items: &[Item<'_>],
        composition: (u32, u32),
        quads: &[Quad],
        masks: &[Mask],
        post: &Post,
    ) -> Result<(), RenderError> {
        if self.surface.is_none() {
            return Ok(());
        }
        let staged = self.stage(items, composition, quads, masks, post);
        let Some(surface) = &self.surface else {
            return Ok(());
        };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Validation => {
                surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => return Err(RenderError::Lost),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.draw_composition(
            &mut encoder,
            &staged.steps,
            staged.program.clone(),
            post,
            composition,
        );
        self.present_pass(&mut encoder, &view, items, staged.result);
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }

    /// Uploads the pictures and writes the vertices, masks and effects of a frame: what the
    /// passes of [`Self::draw_composition`] and [`Self::present_pass`] read.
    fn stage(
        &mut self,
        items: &[Item<'_>],
        composition: (u32, u32),
        quads: &[Quad],
        masks: &[Mask],
        post: &Post,
    ) -> Staged {
        self.queue.write_buffer(
            &self.masks,
            0,
            &mask_bytes(masks, (self.config.width, self.config.height)),
        );
        let fitted;
        let quads = if quads.is_empty() {
            fitted = [fitted_quad(
                composition,
                (self.config.width, self.config.height),
            )];
            &fitted[..]
        } else {
            &quads[..quads.len().min(MAX_QUADS)]
        };
        // The pictures always go into the composition's textures, each over the whole of it: every
        // blend reads the light below, and the quads and masks cut the result afterwards.
        if post.is_empty()
            && let Some(compose) = &mut self.compose
        {
            compose.forget();
        }
        let whole = [whole_quad()];
        let into = &whole[..];
        // Each picture's vertices, and the range of them it draws.
        let mut data: Vec<Vertex> = Vec::new();
        let mut steps: Vec<Step> = Vec::new();
        let mut effects = [Effects::default().pack(); MAX_LAYERS];
        // The textures are per picture, from the bottom; the pictures of the bottom are the ones
        // left out when there are too many.
        let mut skip = items
            .iter()
            .map(Item::count)
            .sum::<usize>()
            .saturating_sub(MAX_LAYERS);
        let mut slot = 0;
        for item in items {
            let (pictures, group) = match item {
                Item::Picture(picture) => (std::slice::from_ref(picture), None),
                Item::Group(group) => (&group.pictures[..], Some(group)),
            };
            let mut staged = Vec::new();
            for picture in pictures {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                if slot < MAX_LAYERS && picture.opacity > 0.0 && self.upload(slot, *picture) {
                    let first = data.len() as u32;
                    data.extend(into.iter().flat_map(vertices).map(|vertex| Vertex {
                        place: picture.place,
                        opacity: picture.opacity.min(1.0),
                        shading: picture.blend.shading(),
                        fx: slot as f32,
                        ..vertex
                    }));
                    effects[slot] = picture.effects.pack();
                    staged.push((slot, first..data.len() as u32));
                }
                slot += 1;
            }
            match group {
                None => steps.extend(staged.pop().map(|(slot, range)| Step::Picture(slot, range))),
                Some(group) if !staged.is_empty() && slot < MAX_LAYERS && group.opacity > 0.0 => {
                    // The group's mix is drawn whole, with its own effects in the slot after its
                    // pictures'.
                    let at = slot;
                    slot += 1;
                    let first = data.len() as u32;
                    data.extend(into.iter().flat_map(vertices).map(|vertex| Vertex {
                        place: [0.0, 0.0, 1.0, 1.0],
                        opacity: group.opacity.min(1.0),
                        shading: group.blend.shading(),
                        fx: at as f32,
                        ..vertex
                    }));
                    effects[at] = group.effects.pack();
                    steps.push(Step::Group(staged, first..data.len() as u32));
                }
                Some(_) => {}
            }
        }
        // The composition's program and the composition on the window: one picture the size of
        // the composition each.
        let first = data.len() as u32;
        data.extend(vertices(&whole[0]));
        let program = first..data.len() as u32;
        let first = data.len() as u32;
        data.extend(quads.iter().flat_map(vertices));
        let result = first..data.len() as u32;
        self.queue
            .write_buffer(&self.effects, 0, &effect_bytes(&effects));
        if !data.is_empty() {
            self.queue
                .write_buffer(&self.vertices, 0, &vertex_bytes(&data));
        }
        Staged {
            steps,
            program,
            result,
        }
    }

    /// Records the pass that draws the composition's result onto `view` through the quads.
    fn present_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        items: &[Item<'_>],
        result: std::ops::Range<u32>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("picture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        if let Some(compose) = &self.compose
            && let Some(group) = compose.result(items.iter().any(Item::nearest))
        {
            pass.set_bind_group(1, &compose.window_group, &[]);
            pass.set_pipeline(&self.present);
            pass.set_bind_group(0, group, &[]);
            pass.draw(result, 0..1);
        }
    }

    /// Records the passes that draw the pictures into the composition and apply `post` to it.
    fn draw_composition(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        steps: &[Step],
        program: std::ops::Range<u32>,
        post: &Post,
        composition: (u32, u32),
    ) {
        if self.compose.is_none() {
            let shared = compose::Shared {
                masks_layout: &self.masks_layout,
                masks: &self.masks,
                effects: &self.effects,
            };
            let (device, shader, layouts) = (&self.device, &self.shader, &self.pipeline_layouts);
            self.compose = Some(compose::Compose::new(
                device,
                self.composition_format,
                &shared,
                |format, reads| picture_pipeline(device, shader, layouts, format, reads),
            ));
        }
        let Some(compose) = self.compose.as_mut() else {
            return;
        };
        let textures = &self.pictures;
        let texture = |slot: &usize| Some(&textures[*slot].as_ref()?.bind_group);
        let groups: Vec<_> = steps
            .iter()
            .filter_map(|step| match step {
                Step::Picture(slot, range) => {
                    Some(compose::Drawn::Picture(texture(slot)?, range.clone()))
                }
                Step::Group(pictures, range) => Some(compose::Drawn::Group(
                    pictures
                        .iter()
                        .filter_map(|(slot, range)| Some((texture(slot)?, range.clone())))
                        .collect(),
                    range.clone(),
                )),
            })
            .collect();
        compose.run(
            compose::Inputs {
                device: &self.device,
                queue: &self.queue,
                encoder,
                vertices: &self.vertices,
                layers: &groups,
                program,
                post,
                composition,
            },
            &self.sampler,
            &self.layout,
        );
    }

    /// Puts `picture` in the texture of layer `slot`; `false` when it cannot be shown.
    fn upload(&mut self, slot: usize, picture: Picture<'_>) -> bool {
        let expected = picture.width as usize * picture.height as usize * 4;
        if picture.width == 0 || picture.height == 0 || picture.rgba.len() != expected {
            return false;
        }
        if self.pictures[slot]
            .as_ref()
            .is_some_and(|p| p.id == picture.id)
        {
            return true;
        }
        let size = wgpu::Extent3d {
            width: picture.width,
            height: picture.height,
            depth_or_array_layers: 1,
        };
        let reuse = self.pictures[slot].take().filter(|p| {
            (p.width, p.height, p.nearest) == (picture.width, picture.height, picture.nearest)
        });
        let uploaded = reuse.unwrap_or_else(|| {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("picture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("picture"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(if picture.nearest {
                            &self.nearest
                        } else {
                            &self.sampler
                        }),
                    },
                ],
            });
            Uploaded {
                id: picture.id,
                width: picture.width,
                height: picture.height,
                nearest: picture.nearest,
                texture,
                bind_group,
            }
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &uploaded.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            picture.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(picture.width * 4),
                rows_per_image: Some(picture.height),
            },
            size,
        );
        self.pictures[slot] = Some(Uploaded {
            id: picture.id,
            ..uploaded
        });
        true
    }
}

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
fn diagonal_weights(p: &[[f32; 2]; 4]) -> [f32; 4] {
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

fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8> {
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

fn effect_bytes(effects: &[[f32; effects::FX_FLOATS]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(effects.len() * effects::FX_FLOATS * 4);
    bytes.extend(effects.iter().flatten().flat_map(|f| f.to_ne_bytes()));
    bytes
}

/// The masks' uniform for a window of `window` pixels: at most [`MAX_MASKS`] masks of 3 to
/// [`MAX_MASK_POINTS`] points; others are left out.
fn mask_bytes(masks: &[Mask], window: (u32, u32)) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use super::{
        MASKS_SIZE, Mask, Quad, SHADER, Sizing, VERTEX_SIZE, fit, fitted_quad, mask_bytes,
        placement, transformed, vertex_bytes, vertices,
    };

    #[test]
    fn placement_should_size_the_picture_into_the_composition() {
        let (wide, full) = ((1920, 1080), (3840, 1080));
        assert_eq!(placement(Sizing::Stretch, wide, full), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(placement(Sizing::Fit, wide, full), [0.25, 0.0, 0.5, 1.0]);
        assert_eq!(
            placement(Sizing::None, (960, 540), full),
            [0.375, 0.25, 0.25, 0.5]
        );
        let fill = placement(Sizing::Fill, wide, full);
        assert_eq!((fill[0], fill[2]), (0.0, 1.0));
        assert!((fill[3] - 2.0).abs() < 1e-6 && (fill[1] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn transformed_should_scale_about_the_centre_and_move_it() {
        let place = [0.25, 0.0, 0.5, 1.0];
        assert_eq!(transformed(place, 1.0, [0.0, 0.0]), place);
        assert_eq!(transformed(place, 2.0, [0.0, 0.0]), [0.0, -0.5, 1.0, 2.0]);
        assert_eq!(
            transformed(place, 0.5, [0.25, -0.25]),
            [0.625, 0.0, 0.25, 0.5]
        );
    }

    #[test]
    fn the_shader_should_parse_and_validate() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("the shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the shader validates");
    }

    #[test]
    fn the_shader_should_know_every_kind_of_step_and_size_the_uniform_as_the_program() {
        use super::{EFFECTS_SIZE, MAX_LAYERS, effects};
        let kinds = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 20, 21, 22, 23, 24, 30, 31,
            32, 33, 34, 35, 36, 37, 38,
        ];
        for kind in kinds {
            assert!(SHADER.contains(&format!("kind == {kind}u")), "kind {kind}");
        }
        assert!(SHADER.contains(&format!("array<vec4f, {}>", effects::MAX_STEPS * 4)));
        assert!(SHADER.contains(&format!("array<Fx, {MAX_LAYERS}>")));
        // A uniform binding holds 64 KiB at least on every GPU wgpu runs on by default.
        const { assert!(EFFECTS_SIZE <= 65_536) };
        assert_eq!(effects::FX_FLOATS * 4, 16 + 16 * 4 * effects::MAX_STEPS);
    }

    #[test]
    fn fit_should_letterbox_a_wide_picture_and_pillarbox_a_tall_one() {
        assert_eq!(fit((1920, 1080), (1920, 1200)), (0.0, 60.0, 1920.0, 1080.0));
        assert_eq!(
            fit((1080, 1080), (1920, 1080)),
            (420.0, 0.0, 1080.0, 1080.0)
        );
        assert_eq!(fit((1280, 720), (1920, 1080)), (0.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn a_rectangle_should_have_no_perspective() {
        let quad = fitted_quad((1080, 1080), (1920, 1080));
        assert_eq!(quad.corners[0], [420.0 / 1920.0, 0.0]);
        for v in vertices(&quad) {
            assert!((v.uvq[2] - 2.0).abs() < 1e-5, "{v:?}");
        }
    }

    #[test]
    fn a_trapezoid_should_weigh_its_wide_side_more() {
        // The top edge is half as wide as the bottom: the picture is seen leaning away.
        let quad = Quad {
            input: [0.0, 0.0, 1.0, 1.0],
            corners: [[0.25, 0.0], [0.75, 0.0], [1.0, 1.0], [0.0, 1.0]],
            soft: [0.0; 4],
            gamma: 1.0,
        };
        let v = vertices(&quad);
        // The narrow top is far away: its corners (0, 1) weigh less than the bottom's (2, 3).
        assert!(v[0].uvq[2] < v[2].uvq[2]);
        // Divided out, every corner still samples its own corner of the picture.
        let uv = |i: usize| [v[i].uvq[0] / v[i].uvq[2], v[i].uvq[1] / v[i].uvq[2]];
        assert_eq!(
            (uv(0), uv(1), uv(2), uv(5)),
            ([0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0])
        );
    }

    #[test]
    fn the_masks_should_fill_their_uniform_exactly() {
        let square = Mask {
            points: vec![[0.1, 0.1], [0.2, 0.1], [0.2, 0.2], [0.1, 0.2]],
            invert: true,
        };
        let line = Mask {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
            invert: false,
        };
        let bytes = mask_bytes(&[line, square], (1920, 1080));
        assert_eq!(bytes.len() as u64, MASKS_SIZE);
        let float = |at: usize| f32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap());
        let int = |at: usize| u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap());
        // A line is no shape: only the square is drawn, from the first point, inverted.
        assert_eq!((float(0), float(4), float(8)), (1920.0, 1080.0, 1.0));
        assert_eq!((int(16), int(20), int(24)), (0, 4, 1));
        assert_eq!(float(16 + 16 * 8 + 8), 0.2);
    }

    #[test]
    fn a_vertex_should_carry_its_place_in_the_quad() {
        let quad = Quad {
            soft: [0.25, 0.0, 0.0, 2.0],
            gamma: 2.2,
            ..fitted_quad((1920, 1080), (1920, 1080))
        };
        let v = vertices(&quad);
        assert_eq!(
            [v[2].local[0] / v[2].uvq[2], v[2].local[1] / v[2].uvq[2]],
            [1.0, 1.0]
        );
        // A fade past the whole quad is cut to it.
        assert_eq!((v[0].soft, v[0].gamma), ([0.25, 0.0, 0.0, 1.0], 2.2));
        assert_eq!(vertex_bytes(&v).len(), 6 * 19 * 4);
        assert_eq!(VERTEX_SIZE, 19 * 4);
    }
}
