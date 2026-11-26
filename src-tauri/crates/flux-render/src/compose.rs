//! The GPU side of the composition's effects (see [`crate::post`]): the textures the pictures are
//! drawn into, the history, the passes of the effects and the bloom's chain of blurs.
//!
//! A frame with composition effects is drawn in passes, all but the last into textures of the
//! composition's size:
//!
//! 1. every picture, with its own effects, blend and opacity, into the first work texture;
//! 2. each history effect (feedback, trails) in turn, reading the work texture and the history
//!    and drawing into the other work texture, which then is the current one;
//! 3. the program of the other effects, as one picture the size of the composition;
//! 4. each bloom: the bright pass, the chain of blurs and the glow added to the current texture;
//! 5. the result copied into the history, for the next frame;
//! 6. the result drawn onto the window through the slices and masks, exactly as a picture is when
//!    there are no effects (that is [`crate::Screen::draw`]'s).

use std::ops::Range;

use crate::post::{BLOOM_LEVELS, Post, Stage, bloom_gains};
use crate::{Blend, EFFECTS_SIZE, MASKS_SIZE, MAX_LAYERS, effects};

const SHADER: &str = include_str!("post.wgsl");
/// What a pass's settings take in the uniform: the offsets of a dynamic binding are aligned to this.
const SLOT: u64 = 256;
/// Most passes with settings of one frame: the history effects and each bloom's passes.
const SLOTS: u64 = 64;
/// The numbers of one pass's settings.
const PARAMS: usize = 16;

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
fn level_size(work: (u32, u32), level: usize) -> (u32, u32) {
    (
        (work.0 >> (level + 1)).max(1),
        (work.1 >> (level + 1)).max(1),
    )
}

struct Image {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

fn image(
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
struct Targets {
    size: (u32, u32),
    /// The two textures the passes draw from one to the other.
    work: [Image; 2],
    history: Image,
    /// The levels of a bloom's chain.
    levels: Vec<Image>,
    /// A work texture as a picture (for the picture shader).
    as_picture: [wgpu::BindGroup; 2],
    /// The same, sampled without smoothing, for a window that shows a test card.
    as_picture_crisp: [wgpu::BindGroup; 2],
    /// A work texture and the history, for the passes that combine them.
    with_history: [wgpu::BindGroup; 2],
    /// A work texture and the first level of a bloom's chain.
    with_bloom: [wgpu::BindGroup; 2],
    /// A work texture on its own, and each level of the chain.
    work_alone: [wgpu::BindGroup; 2],
    level_alone: Vec<wgpu::BindGroup>,
}

/// What a draw of a frame with effects needs from the screen.
pub(crate) struct Inputs<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub vertices: &'a wgpu::Buffer,
    /// The pictures: their texture's group, blend and the vertices that draw them.
    pub layers: &'a [(&'a wgpu::BindGroup, Blend, Range<u32>)],
    /// The vertices of the program's picture.
    pub program: Range<u32>,
    pub post: &'a Post,
    pub composition: (u32, u32),
}

pub(crate) struct Compose {
    format: wgpu::TextureFormat,
    post_layout: wgpu::BindGroupLayout,
    feedback: wgpu::RenderPipeline,
    trails: wgpu::RenderPipeline,
    bright: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    mix: wgpu::RenderPipeline,
    /// The picture shader's pipeline drawing into the work textures with the light below, and the
    /// one drawing a picture plainly over a cleared one.
    picture: wgpu::RenderPipeline,
    plain: wgpu::RenderPipeline,
    params: wgpu::Buffer,
    /// Reads a texel as it is, with no blend between texels.
    nearest: wgpu::Sampler,
    /// The groups of the picture shader's second binding group: the pictures' own effects with no
    /// masks, the program's with no masks, and the masks of the window with no effects.
    pub(crate) pictures_group: wgpu::BindGroup,
    program_group: wgpu::BindGroup,
    pub(crate) window_group: wgpu::BindGroup,
    program_effects: wgpu::Buffer,
    targets: Option<Targets>,
    /// Whether the history holds the frame before.
    history_valid: bool,
    /// The texture of the work pair the last draw ended in.
    current: usize,
}

/// What the groups of the picture shader's second binding are made of.
pub(crate) struct Shared<'a> {
    pub masks_layout: &'a wgpu::BindGroupLayout,
    pub masks: &'a wgpu::Buffer,
    pub effects: &'a wgpu::Buffer,
}

impl Compose {
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        shared: &Shared<'_>,
        make: impl Fn(wgpu::TextureFormat, bool) -> wgpu::RenderPipeline,
    ) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let texture = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composition passes"),
            entries: &[
                entry(0, texture),
                entry(1, texture),
                entry(
                    2,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(
                    3,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new((PARAMS * 4) as u64),
                    },
                ),
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composition passes"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composition passes"),
            bind_group_layouts: &[Some(&post_layout)],
            immediate_size: 0,
        });
        let pass = |entry_point: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry_point),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // The glow is added to what the level holds, and its alpha is left as it is.
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composition passes"),
            size: SLOT * SLOTS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // No masks: the pictures are drawn into the composition, which the masks cut afterwards. A
        // new buffer is zeros, whose count of masks is none.
        let no_masks = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("no masks"),
            size: MASKS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });
        let program_effects = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composition program"),
            size: EFFECTS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // A new buffer is zeros, which is no effect at all.
        let no_effects = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("no effects"),
            size: EFFECTS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: false,
        });
        let group = |label, masks: &wgpu::Buffer, effects: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: shared.masks_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: masks.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: effects.as_entire_binding(),
                    },
                ],
            })
        };
        Self {
            format,
            feedback: pass("fs_feedback", None),
            trails: pass("fs_trails", None),
            bright: pass("fs_bright", None),
            down: pass("fs_down", None),
            up: pass("fs_up", Some(add)),
            mix: pass("fs_mix", None),
            picture: make(format, true),
            plain: make(format, false),
            params,
            nearest: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("composition crisp"),
                ..Default::default()
            }),
            pictures_group: group("composition pictures", &no_masks, shared.effects),
            program_group: group("composition program", &no_masks, &program_effects),
            window_group: group("composition on the window", shared.masks, &no_effects),
            program_effects,
            post_layout,
            targets: None,
            history_valid: false,
            current: 0,
        }
    }

    /// Forgets the history: the frame before is not drawn again by the next frame with effects.
    pub(crate) fn forget(&mut self) {
        self.history_valid = false;
    }

    /// The group of the work texture the last draw ended in, drawn as a picture.
    pub(crate) fn result(&self, crisp: bool) -> Option<&wgpu::BindGroup> {
        let targets = self.targets.as_ref()?;
        Some(if crisp {
            &targets.as_picture_crisp[self.current]
        } else {
            &targets.as_picture[self.current]
        })
    }

    fn make_targets(
        &self,
        device: &wgpu::Device,
        sampler: &wgpu::Sampler,
        picture_layout: &wgpu::BindGroupLayout,
        size: (u32, u32),
    ) -> Targets {
        use wgpu::TextureUsages as U;
        let work = [0, 1].map(|n| {
            image(
                device,
                self.format,
                size,
                U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC | U::COPY_DST,
                &format!("composition {n}"),
            )
        });
        let history = image(
            device,
            self.format,
            size,
            U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_DST,
            "composition history",
        );
        let levels: Vec<Image> = (0..BLOOM_LEVELS)
            .map(|level| {
                image(
                    device,
                    self.format,
                    level_size(size, level),
                    U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
                    "bloom",
                )
            })
            .collect();
        let pair = |a: &wgpu::TextureView, b: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("composition pass"),
                layout: &self.post_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(a),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(b),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.params,
                            offset: 0,
                            size: wgpu::BufferSize::new((PARAMS * 4) as u64),
                        }),
                    },
                ],
            })
        };
        let picture = |view: &wgpu::TextureView, sampler: &wgpu::Sampler| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("composition"),
                layout: picture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        let (w0, w1) = (&work[0].view, &work[1].view);
        Targets {
            size,
            as_picture: [picture(w0, sampler), picture(w1, sampler)],
            as_picture_crisp: [picture(w0, &self.nearest), picture(w1, &self.nearest)],
            with_history: [pair(w0, &history.view), pair(w1, &history.view)],
            with_bloom: [pair(w0, &levels[0].view), pair(w1, &levels[0].view)],
            work_alone: [pair(w0, w0), pair(w1, w1)],
            level_alone: levels.iter().map(|l| pair(&l.view, &l.view)).collect(),
            work,
            history,
            levels,
        }
    }

    /// Draws the frame's passes up to the result, which [`Self::result`] then gives; the next frame
    /// reads it from the history. `sampler` and `picture_layout` are the screen's.
    pub(crate) fn run(
        &mut self,
        inputs: Inputs<'_>,
        sampler: &wgpu::Sampler,
        picture_layout: &wgpu::BindGroupLayout,
    ) {
        let Inputs {
            device,
            queue,
            encoder,
            vertices,
            layers,
            program,
            post,
            composition,
        } = inputs;
        let size = work_size(composition, device.limits().max_texture_dimension_2d);
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            self.targets = Some(self.make_targets(device, sampler, picture_layout, size));
            self.history_valid = false;
        }
        let Some(targets) = self.targets.as_ref() else {
            return;
        };

        // The settings of each pass, in the order they are drawn.
        let mut settings: Vec<[f32; PARAMS]> = Vec::new();
        let mut setting = |values: &[f32]| -> u32 {
            let mut slot = [0.0; PARAMS];
            slot[..values.len()].copy_from_slice(values);
            settings.push(slot);
            ((settings.len() as u64 - 1) * SLOT) as u32
        };
        let aspect = size.0 as f32 / size.1 as f32;

        if !self.history_valid {
            clear(encoder, &targets.history.view);
        }

        // 1. The pictures, one pass for each: what is drawn so far is copied into the other
        // texture, the picture is drawn there with the first as the light below, and the other
        // texture is the current one from then on.
        let mut current = 0;
        clear(encoder, &targets.work[0].view);
        for (group, _, range) in layers {
            let target = 1 - current;
            encoder.copy_texture_to_texture(
                targets.work[current].texture.as_image_copy(),
                targets.work[target].texture.as_image_copy(),
                wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composition pictures"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.work[target].view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_bind_group(1, &self.pictures_group, &[]);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_bind_group(2, &targets.as_picture[current], &[]);
            pass.set_pipeline(&self.picture);
            pass.set_bind_group(0, *group, &[]);
            pass.draw(range.clone(), 0..1);
            drop(pass);
            current = target;
        }

        // 2. The history effects.
        for stage in &post.stages {
            let step = stage.step(post.dt);
            let (pipeline, offset) = match stage {
                Stage::Feedback(_) => (
                    &self.feedback,
                    setting(&[
                        step.keep,
                        step.scale,
                        step.angle,
                        step.hue,
                        step.shift[0],
                        step.shift[1],
                        step.mode as f32,
                        aspect,
                    ]),
                ),
                Stage::Trails(_) => (
                    &self.trails,
                    setting(&[step.keep, 0.0, 0.0, 0.0, 0.0, 0.0, step.mode as f32, aspect]),
                ),
            };
            let group = &targets.with_history[current];
            fullscreen(
                encoder,
                &targets.work[1 - current].view,
                Some(wgpu::Color::BLACK),
                pipeline,
                group,
                offset,
            );
            current = 1 - current;
        }

        // 3. The program of the other effects.
        if !post.program.is_identity() {
            let mut all = [effects::Effects::default().pack(); MAX_LAYERS];
            all[0] = post.program.pack();
            queue.write_buffer(&self.program_effects, 0, &crate::effect_bytes(&all));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composition program"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.work[1 - current].view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.plain);
            pass.set_bind_group(0, &targets.as_picture[current], &[]);
            pass.set_bind_group(1, &self.program_group, &[]);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(program, 0..1);
            drop(pass);
            current = 1 - current;
        }

        // The history of the next frame is what the history effects and the program made of this one:
        // the blooms are only for the eye, and are not drawn again by a feedback.
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.work[current].texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &targets.history.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );

        // 4. The blooms.
        for bloom in &post.blooms {
            let (gains, levels) = bloom_gains(bloom.radius);
            let texel = |image: usize| -> [f32; 2] {
                let (w, h) = if image == 0 {
                    size
                } else {
                    level_size(size, image - 1)
                };
                [1.0 / w as f32, 1.0 / h as f32]
            };
            let [tx, ty] = texel(0);
            let offset = setting(&[bloom.threshold, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, tx, ty]);
            fullscreen(
                encoder,
                &targets.levels[0].view,
                Some(wgpu::Color::BLACK),
                &self.bright,
                &targets.work_alone[current],
                offset,
            );
            for level in 1..levels {
                let [tx, ty] = texel(level);
                let offset = setting(&[0.0; 8].iter().copied().chain([tx, ty]).collect::<Vec<_>>());
                fullscreen(
                    encoder,
                    &targets.levels[level].view,
                    Some(wgpu::Color::BLACK),
                    &self.down,
                    &targets.level_alone[level - 1],
                    offset,
                );
            }
            for up in (0..levels - 1).rev() {
                // Half a pixel of the smaller image: the level above it is `up + 1`.
                let [tx, ty] = texel(up + 2);
                let offset = setting(
                    &[gains[up], 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
                        .iter()
                        .copied()
                        .chain([tx * 0.5, ty * 0.5])
                        .collect::<Vec<_>>(),
                );
                fullscreen(
                    encoder,
                    &targets.levels[up].view,
                    None,
                    &self.up,
                    &targets.level_alone[up + 1],
                    offset,
                );
            }
            let offset = setting(&[bloom.intensity]);
            fullscreen(
                encoder,
                &targets.work[1 - current].view,
                Some(wgpu::Color::BLACK),
                &self.mix,
                &targets.with_bloom[current],
                offset,
            );
            current = 1 - current;
        }

        let mut bytes = vec![0u8; settings.len() * SLOT as usize];
        for (at, slot) in settings.iter().enumerate() {
            for (i, value) in slot.iter().enumerate() {
                let to = at * SLOT as usize + i * 4;
                bytes[to..to + 4].copy_from_slice(&value.to_ne_bytes());
            }
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.params, 0, &bytes);
        }
        self.history_valid = true;
        self.current = current;
    }
}

fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear history"),
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
}

/// One triangle over the whole of `target`, drawn by `pipeline` with `group` and the settings at
/// `offset`; `clear` is the colour it starts from, or none to draw over what it holds.
fn fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    clear: Option<wgpu::Color>,
    pipeline: &wgpu::RenderPipeline,
    group: &wgpu::BindGroup,
    offset: u32,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("composition pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[offset]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::{TextureFormatFeatureFlags as F, TextureUsages as U};

    #[test]
    fn the_composition_should_be_drawn_in_half_floats_when_the_adapter_can() {
        let all = U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC | U::COPY_DST;
        let flags = F::FILTERABLE | F::BLENDABLE;
        assert_eq!(
            composition_format(all, flags),
            wgpu::TextureFormat::Rgba16Float
        );
        // One thing it cannot do and the sRGB bytes it is.
        for (usages, flags) in [
            (all - U::RENDER_ATTACHMENT, flags),
            (all - U::COPY_DST, flags),
            (all, F::FILTERABLE),
            (all, F::BLENDABLE),
        ] {
            assert_eq!(
                composition_format(usages, flags),
                wgpu::TextureFormat::Rgba8UnormSrgb
            );
        }
    }

    #[test]
    fn the_work_size_should_keep_within_what_the_gpu_holds() {
        assert_eq!(work_size((1920, 1080), 8192), (1920, 1080));
        assert_eq!(work_size((8192, 4096), 4096), (4096, 4096));
        assert_eq!(work_size((0, 0), 8192), (1, 1));
        assert_eq!(level_size((1920, 1080), 0), (960, 540));
        assert_eq!(level_size((1920, 1080), 3), (120, 67));
        assert_eq!(level_size((4, 1), 3), (1, 1));
    }

    #[test]
    fn the_passes_should_parse_and_validate() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("the passes parse");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("the passes validate");
        for entry in [
            "vs",
            "fs_feedback",
            "fs_trails",
            "fs_bright",
            "fs_down",
            "fs_up",
            "fs_mix",
        ] {
            assert!(
                module.entry_points.iter().any(|e| e.name == entry),
                "{entry}"
            );
        }
        // The settings of a pass are four vectors, as many floats as the layout of its binding.
        assert!(SHADER.contains("struct Params"));
        const { assert!(PARAMS == 16) };
    }

    /// A device with no window, when this machine has any adapter at all (a runner may have none).
    fn headless() -> Option<(wgpu::Device, wgpu::Queue, wgpu::TextureFormat)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).ok()?;
        let features = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Float);
        let format = composition_format(features.allowed_usages, features.flags);
        eprintln!(
            "headless adapter: {:?}, {format:?}",
            adapter.get_info().name
        );
        Some((device, queue, format))
    }

    /// Half a float as a float.
    fn half(bits: u16) -> f32 {
        let (sign, exponent, fraction) = (bits >> 15, (bits >> 10) & 0x1f, f32::from(bits & 0x3ff));
        let magnitude = match exponent {
            0 => fraction * 2f32.powi(-24),
            0x1f => f32::INFINITY,
            e => (1.0 + fraction / 1024.0) * 2f32.powi(i32::from(e) - 15),
        };
        if sign == 1 { -magnitude } else { magnitude }
    }

    #[test]
    fn a_composition_with_effects_should_draw_on_a_real_device() {
        use crate::post::{Bloom, Feedback, Trails};
        use crate::{SHADER, Vertex, picture_pipeline, vertex_bytes, vertices, whole_quad};
        use flux_project::vj_effects::{FeedbackBlend, TrailMode};
        use std::sync::{Arc, Mutex};

        let Some((device, queue, format)) = headless() else {
            return;
        };
        let errors = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&errors);
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            sink.lock().unwrap().push(error.to_string());
        }));
        let layout = crate::picture_group_layout(&device);
        let masks_layout = crate::masks_group_layout(&device);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = crate::pipeline_layouts(&device, &layout, &masks_layout);
        let uniform = |size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let (masks, effects) = (uniform(MASKS_SIZE), uniform(EFFECTS_SIZE));
        let shared = Shared {
            masks_layout: &masks_layout,
            masks: &masks,
            effects: &effects,
        };
        let mut compose = Compose::new(&device, format, &shared, |format, reads| {
            picture_pipeline(&device, &shader, &pipeline_layout, format, reads)
        });

        // One white pixel as a picture, in the middle of the composition.
        let white = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            white.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            white.size(),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let picture = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &white.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let whole = vertices(&whole_quad());
        let mut data: Vec<Vertex> = whole
            .iter()
            .map(|v| Vertex {
                place: [0.375, 0.375, 0.25, 0.25],
                ..*v
            })
            .collect();
        data.extend(whole);
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: vertex_bytes(&data).len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, &vertex_bytes(&data));

        let post = Post {
            program: effects::Effects::default(),
            stages: vec![
                Stage::Feedback(Feedback {
                    amount: 0.5,
                    zoom: 1.05,
                    rotate: 10.0,
                    shift: [0.0; 2],
                    hue: 20.0,
                    blend: FeedbackBlend::Mix,
                }),
                Stage::Trails(Trails {
                    decay: 0.9,
                    mode: TrailMode::Lighten,
                }),
            ],
            blooms: vec![Bloom {
                threshold: 0.5,
                intensity: 1.0,
                radius: 0.6,
            }],
            dt: 1.0 / 60.0,
        };
        let size = 64;
        let texel = if format == wgpu::TextureFormat::Rgba16Float {
            8
        } else {
            4
        };
        let row = size * texel;
        // Draws `frames` frames with `post` and reads the last one back, as its red.
        let mut render = |post: &Post,
                          frames: usize,
                          vertex_buffer: &wgpu::Buffer,
                          layers: &[(&wgpu::BindGroup, Blend, Range<u32>)]|
         -> Vec<f32> {
            for _ in 0..frames {
                let mut encoder = device.create_command_encoder(&Default::default());
                compose.run(
                    Inputs {
                        device: &device,
                        queue: &queue,
                        encoder: &mut encoder,
                        vertices: vertex_buffer,
                        layers,
                        program: 6..12,
                        post,
                        composition: (size, size),
                    },
                    &sampler,
                    &layout,
                );
                queue.submit([encoder.finish()]);
            }
            let targets = compose.targets.as_ref().unwrap();
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: u64::from(row * size),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                targets.work[compose.current].texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row),
                        rows_per_image: Some(size),
                    },
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([encoder.finish()]);
            readback.slice(..).map_async(wgpu::MapMode::Read, |result| {
                result.expect("the result maps");
            });
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("the device finishes");
            let bytes = readback
                .slice(..)
                .get_mapped_range()
                .expect("the result is mapped")
                .to_vec();
            bytes
                .chunks(texel as usize)
                .map(|p| {
                    if texel == 8 {
                        half(u16::from_le_bytes([p[0], p[1]]))
                    } else {
                        f32::from(p[0]) / 255.0
                    }
                })
                .collect()
        };
        let at = |image: &[f32], x: usize, y: usize| image[y * size as usize + x];
        let alpha = [(&picture, Blend::Alpha, 0..6)];
        let image = render(&post, 3, &vertex_buffer, &alpha);
        // The picture is lit, the bloom lights beyond it and the far corner stays dark.
        assert!(at(&image, 32, 32) > 0.5, "{}", at(&image, 32, 32));
        assert!(at(&image, 22, 32) > 0.0, "{}", at(&image, 22, 32));
        assert!(at(&image, 0, 0) < 0.01, "{}", at(&image, 0, 0));
        // The program of the other effects, drawn on the whole composition: an invert.
        let invert = flux_project::vj_effects::Effect {
            id: "i".into(),
            bypass: false,
            modulation: Default::default(),
            kind: flux_project::vj_effects::EffectKind::Invert { amount: 1.0 },
        };
        let inverted = Post::new(&[invert], effects::Time::default(), 1.0 / 60.0);
        let image = render(&inverted, 1, &vertex_buffer, &alpha);
        assert!(at(&image, 32, 32) < 0.01, "{}", at(&image, 32, 32));
        assert!(at(&image, 0, 0) > 0.99, "{}", at(&image, 0, 0));

        // A blend that reads the light below: white over the white middle makes it black, over the
        // black around it makes it white, whether it comes first, after a picture or before one.
        let difference = |place: [f32; 4], shading: f32| -> Vec<Vertex> {
            vertices(&whole_quad())
                .into_iter()
                .map(|v| Vertex {
                    place,
                    shading,
                    ..v
                })
                .collect()
        };
        let middle = [0.375, 0.375, 0.25, 0.25];
        let shading = Blend::Difference.shading();
        let mut data = difference(middle, 0.0);
        data.extend(difference([0.0, 0.0, 1.0, 1.0], shading));
        data.extend(difference(middle, 0.0));
        let blends = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: vertex_bytes(&data).len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&blends, 0, &vertex_bytes(&data));
        let none = Post::default();
        let stack = [
            (&picture, Blend::Alpha, 0..6),
            (&picture, Blend::Difference, 6..12),
        ];
        let image = render(&none, 1, &blends, &stack);
        assert!(at(&image, 30, 34) < 0.01, "{}", at(&image, 30, 34));
        assert!(at(&image, 2, 2) > 0.99, "{}", at(&image, 2, 2));
        let first = [(&picture, Blend::Difference, 6..12)];
        let image = render(&none, 1, &blends, &first);
        assert!(
            at(&image, 30, 34) > 0.99 && at(&image, 2, 2) > 0.99,
            "{} {}",
            at(&image, 30, 34),
            at(&image, 2, 2)
        );
        // The picture drawn after it covers the middle again, over the blended light.
        let after = [
            (&picture, Blend::Alpha, 0..6),
            (&picture, Blend::Difference, 6..12),
            (&picture, Blend::Alpha, 12..18),
        ];
        let image = render(&none, 1, &blends, &after);
        assert!(
            at(&image, 30, 34) > 0.99 && at(&image, 2, 2) > 0.99,
            "{} {}",
            at(&image, 30, 34),
            at(&image, 2, 2)
        );

        // Every blend the shader does should come to what the monitor's CPU twin makes of it: a
        // picture of half its colour covering over another, in a composition of nothing else.
        let texel = |rgba: [u8; 4]| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                texture.size(),
            );
            let view = texture.create_view(&Default::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            })
        };
        let (below, above) = ([200, 100, 50, 255], [60, 180, 220, 192]);
        let (below_group, above_group) = (texel(below), texel(above));
        // The blends work on the encoded values, which are the bytes; the composition holds light.
        let encoded = |rgba: [u8; 4]| [0, 1, 2].map(|c| f32::from(rgba[c]) / 255.0);
        let cover = f32::from(above[3]) / 255.0;
        // The red of the result is all it reads, in light: each blend over the same below.
        let blends: Vec<Blend> = Blend::ALL.into_iter().collect();
        for blend in blends {
            let mut data = difference([0.0, 0.0, 1.0, 1.0], 0.0);
            data.extend(difference([0.0, 0.0, 1.0, 1.0], blend.shading()));
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: vertex_bytes(&data).len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buffer, 0, &vertex_bytes(&data));
            let layers = [
                (&below_group, Blend::Alpha, 0..6),
                (&above_group, blend, 6..12),
            ];
            let image = render(&none, 1, &buffer, &layers);
            let expected = crate::composite::to_linear(
                crate::composite::blend_light(
                    blend,
                    encoded(above),
                    cover,
                    1.0,
                    encoded(below),
                    encoded(below),
                )[0],
            );
            assert!(
                (at(&image, 10, 10) - expected).abs() < 0.01,
                "{blend:?}: {} against {}",
                at(&image, 10, 10),
                expected
            );
        }
        assert_eq!(errors.lock().unwrap().as_slice(), &[] as &[String]);
    }
}
