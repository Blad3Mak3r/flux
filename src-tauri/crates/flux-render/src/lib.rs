//! Draws video frames on an output window through the GPU (wgpu): the picture is fitted to the
//! window keeping its shape, with black around it, and the window is black when there is none.
//!
//! The renderer is one window and one picture for now. Composition of several layers, slices of the
//! picture and several outputs (the Resolume-style mapping) build on the same device later.

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
}

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
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    picture: Option<Uploaded>,
    errors: Errors,
}

/// The last error the GPU reported while drawing, shared with whoever reports it. A GPU error never
/// panics: the picture it hit is lost and drawing goes on.
pub type Errors = std::sync::Arc<std::sync::Mutex<Option<String>>>;

const SHADER: &str = r"
struct Out {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};

// One triangle that covers the viewport; the viewport is where the picture goes.
@vertex
fn vs(@builtin(vertex_index) i: u32) -> Out {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.pos = vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@group(0) @binding(0) var picture: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;

@fragment
fn fs(in: Out) -> @location(0) vec4f {
    return textureSample(picture, linear_sampler, in.uv);
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("picture"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("picture"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
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
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            layout,
            sampler,
            picture: None,
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

    /// Draws `picture` fitted to the window, or black for `None`. Waits for the display's next
    /// refresh, so a loop calling it runs at the display's rate.
    pub fn draw(&mut self, picture: Option<Picture<'_>>) -> Result<(), RenderError> {
        if let Some(picture) = picture {
            self.upload(picture);
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
            if let Some(uploaded) = picture.and(self.picture.as_ref()) {
                let (x, y, w, h) = fit(
                    (uploaded.width, uploaded.height),
                    (self.config.width, self.config.height),
                );
                pass.set_viewport(x, y, w, h, 0.0, 1.0);
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &uploaded.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }

    fn upload(&mut self, picture: Picture<'_>) {
        let expected = picture.width as usize * picture.height as usize * 4;
        if picture.width == 0 || picture.height == 0 || picture.rgba.len() != expected {
            return;
        }
        if self.picture.as_ref().is_some_and(|p| p.id == picture.id) {
            return;
        }
        let size = wgpu::Extent3d {
            width: picture.width,
            height: picture.height,
            depth_or_array_layers: 1,
        };
        let reuse = self
            .picture
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
        self.picture = Some(Uploaded {
            id: picture.id,
            ..uploaded
        });
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

#[cfg(test)]
mod tests {
    use super::{SHADER, fit};

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
}
