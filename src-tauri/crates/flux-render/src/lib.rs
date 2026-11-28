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

mod geometry;
mod pipelines;
mod types;

pub use geometry::*;
use pipelines::*;
pub use types::*;

#[cfg(test)]
mod tests;

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
