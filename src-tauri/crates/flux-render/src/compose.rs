//! The GPU side of the composition's effects (see [`crate::post`]): the textures the pictures are
//! drawn into, the history, the passes of the effects and the bloom's chain of blurs.
//!
//! A frame with composition effects is drawn in passes, all but the last into textures of the
//! composition's size:
//!
//! 1. every picture, with its own effects, blend and opacity, into the first work texture; the
//!    pictures of a group are first mixed together over a clear texture of their own, and that
//!    one is then drawn like a picture, with the group's effects, blend and opacity;
//! 2. each history effect (feedback, trails) in turn, reading the work texture and the history
//!    and drawing into the other work texture, which then is the current one;
//! 3. the program of the other effects, as one picture the size of the composition;
//! 4. each bloom: the bright pass, the chain of blurs and the glow added to the current texture;
//! 5. the result copied into the history, for the next frame;
//! 6. the result drawn onto the window through the slices and masks, exactly as a picture is when
//!    there are no effects (that is [`crate::Screen::draw`]'s).

use std::ops::Range;

use crate::post::{BLOOM_LEVELS, Post, Stage, bloom_gains};
use crate::{EFFECTS_SIZE, MASKS_SIZE, MAX_LAYERS, effects};

mod helpers;
mod targets;

use helpers::*;
pub(crate) use targets::*;

#[cfg(test)]
mod tests;

const SHADER: &str = include_str!("post.wgsl");
/// What a pass's settings take in the uniform: the offsets of a dynamic binding are aligned to this.
const SLOT: u64 = 256;
/// Most passes with settings of one frame: the history effects and each bloom's passes.
const SLOTS: u64 = 64;
/// The numbers of one pass's settings.
const PARAMS: usize = 16;

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
        let inner = [0, 1].map(|n| {
            image(
                device,
                self.format,
                size,
                U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC | U::COPY_DST,
                &format!("group {n}"),
            )
        });
        let (w0, w1) = (&work[0].view, &work[1].view);
        Targets {
            size,
            as_picture: [picture(w0, sampler), picture(w1, sampler)],
            as_picture_crisp: [picture(w0, &self.nearest), picture(w1, &self.nearest)],
            inner_picture: [
                picture(&inner[0].view, sampler),
                picture(&inner[1].view, sampler),
            ],
            inner,
            with_history: [pair(w0, &history.view), pair(w1, &history.view)],
            with_bloom: [pair(w0, &levels[0].view), pair(w1, &levels[0].view)],
            work_alone: [pair(w0, w0), pair(w1, w1)],
            level_alone: levels.iter().map(|l| pair(&l.view, &l.view)).collect(),
            work,
            history,
            levels,
        }
    }

    /// Draws the picture whose texture `group` is, by the vertices `range`, into `pair.to` over the
    /// light in `pair.from`.
    fn over(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        vertices: &wgpu::Buffer,
        size: (u32, u32),
        pair: Pair<'_>,
        group: &wgpu::BindGroup,
        range: Range<u32>,
    ) {
        encoder.copy_texture_to_texture(
            pair.from.texture.as_image_copy(),
            pair.to.texture.as_image_copy(),
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composition pictures"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &pair.to.view,
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
        pass.set_bind_group(2, pair.backdrop, &[]);
        pass.set_pipeline(&self.picture);
        pass.set_bind_group(0, group, &[]);
        pass.draw(range, 0..1);
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
        // texture is the current one from then on. A group's pictures are drawn so in the pair of
        // its own, from clear; the mix is then one more picture of the composition's.
        let mut current = 0;
        clear(encoder, &targets.work[0].view);
        for step in layers {
            let (group, range) = match step {
                Drawn::Picture(group, range) => (*group, range),
                Drawn::Group(pictures, range) => {
                    let mut inner = 0;
                    clear_to(encoder, &targets.inner[0].view, wgpu::Color::TRANSPARENT);
                    for (group, range) in pictures {
                        let pair = Pair {
                            from: &targets.inner[inner],
                            to: &targets.inner[1 - inner],
                            backdrop: &targets.inner_picture[inner],
                        };
                        self.over(encoder, vertices, size, pair, group, range.clone());
                        inner = 1 - inner;
                    }
                    (&targets.inner_picture[inner], range)
                }
            };
            let pair = Pair {
                from: &targets.work[current],
                to: &targets.work[1 - current],
                backdrop: &targets.as_picture[current],
            };
            self.over(encoder, vertices, size, pair, group, range.clone());
            current = 1 - current;
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
