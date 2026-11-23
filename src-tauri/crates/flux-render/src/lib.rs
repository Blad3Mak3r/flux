//! Draws video frames on an output window through the GPU (wgpu). The picture reaches the window as
//! [`Quad`]s: each takes a rectangle of the picture and stretches it between four corners of the
//! window, with the perspective of a projector seen at an angle (a "corner pin"). With no quads the
//! whole picture is fitted to the window keeping its shape, with black around it; the window is
//! black when there is no picture. Several pictures are drawn one over another, each at its own
//! place and opacity, which is how clips fade and cross-fade. A quad can fade out towards its edges (a soft edge, to blend
//! two projectors where their pictures overlap), and [`Mask`]s black out parts of the window.

pub mod pattern;

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
pub const MAX_LAYERS: usize = 4;

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
}

const VERTEX_SIZE: u64 = size_of::<Vertex>() as u64;

struct Uploaded {
    id: u64,
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// A window's GPU surface, made before the device that draws on it.
pub struct Target {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
}

/// A window the pictures are drawn on.
pub struct Screen {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    masks: wgpu::Buffer,
    masks_group: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The texture of each layer, from the bottom.
    pictures: Vec<Option<Uploaded>>,
    errors: Errors,
}

/// The last error the GPU reported while drawing, shared with whoever reports it. A GPU error never
/// panics: the picture it hit is lost and drawing goes on.
pub type Errors = std::sync::Arc<std::sync::Mutex<Option<String>>>;

const SHADER: &str = r"
struct Out {
    @builtin(position) pos: vec4f,
    @location(0) uvq: vec3f,
    @location(1) local: vec2f,
    @location(2) @interpolate(flat) soft: vec4f,
    @location(3) @interpolate(flat) gamma: f32,
    @location(4) @interpolate(flat) place: vec4f,
    @location(5) @interpolate(flat) opacity: f32,
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
) -> Out {
    var out: Out;
    out.pos = vec4f(pos, 0.0, 1.0);
    out.uvq = uvq;
    out.local = local;
    out.soft = soft;
    out.gamma = gamma;
    out.place = place;
    out.opacity = opacity;
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
    let colour = textureSample(picture, linear_sampler, at);
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
    return vec4f(colour.rgb * shown, cover);
}
";

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
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("flux-render"),
                ..Default::default()
            }))?;
        let errors = Errors::default();
        let sink = std::sync::Arc::clone(&errors);
        device.on_uncaptured_error(std::sync::Arc::new(move |error: wgpu::Error| {
            *sink
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
        }));

        let caps = surface.get_capabilities(&adapter);
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

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("picture"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let masks_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("masks"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let masks = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("masks"),
            size: MASKS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let masks_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("masks"),
            layout: &masks_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: masks.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("picture"),
            bind_group_layouts: &[Some(&layout), Some(&masks_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("picture"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
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
                        6 => Float32
                    ],
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("picture"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
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
            pipeline,
            vertices,
            masks,
            masks_group,
            layout,
            sampler,
            pictures: (0..MAX_LAYERS).map(|_| None).collect(),
            errors,
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
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Draws `pictures` one over another from the first (at most [`MAX_LAYERS`], the top ones
    /// kept), each placed in a composition of `composition` pixels, through `quads` (at most
    /// [`MAX_QUADS`]) or with the composition fitted to the window when there are none, with
    /// `masks` hiding parts of it; black for no picture. Waits for the display's next refresh, so
    /// a loop calling it runs at the display's rate.
    pub fn draw(
        &mut self,
        pictures: &[Picture<'_>],
        composition: (u32, u32),
        quads: &[Quad],
        masks: &[Mask],
    ) -> Result<(), RenderError> {
        self.queue.write_buffer(
            &self.masks,
            0,
            &mask_bytes(masks, (self.config.width, self.config.height)),
        );
        let pictures = &pictures[pictures.len().saturating_sub(MAX_LAYERS)..];
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
        // Each layer's vertices, and the range of them it draws.
        let mut data: Vec<Vertex> = Vec::new();
        let mut layers = Vec::new();
        for (slot, picture) in pictures.iter().enumerate() {
            if picture.opacity <= 0.0 || !self.upload(slot, *picture) {
                continue;
            }
            let first = data.len() as u32;
            data.extend(quads.iter().flat_map(vertices).map(|vertex| Vertex {
                place: picture.place,
                opacity: picture.opacity.min(1.0),
                ..vertex
            }));
            layers.push((slot, first..data.len() as u32));
        }
        if !data.is_empty() {
            self.queue
                .write_buffer(&self.vertices, 0, &vertex_bytes(&data));
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Validation => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => return Err(RenderError::Lost),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("picture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(1, &self.masks_group, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            for (slot, range) in layers {
                if let Some(uploaded) = &self.pictures[slot] {
                    pass.set_bind_group(0, &uploaded.bind_group, &[]);
                    pass.draw(range, 0..1);
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
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
        let reuse = self.pictures[slot]
            .take()
            .filter(|p| (p.width, p.height) == (picture.width, picture.height));
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
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            Uploaded {
                id: picture.id,
                width: picture.width,
                height: picture.height,
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
                .chain([&v.opacity])
                .flat_map(|f| f.to_ne_bytes())
        })
        .collect()
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
        placement, vertex_bytes, vertices,
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
        assert_eq!(vertex_bytes(&v).len(), 6 * 17 * 4);
        assert_eq!(VERTEX_SIZE, 17 * 4);
    }
}
