use super::*;
use crate::Blend;
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
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).ok()?;
    let features = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Float);
    let format = composition_format(features.allowed_usages, features.flags);
    eprintln!(
        "headless adapter: {:?}, {format:?}",
        adapter.get_info().name
    );
    Some((device, queue, format))
}

/// The pictures of `layers`, each a step of the first pass.
fn drawn<'a>(layers: &[(&'a wgpu::BindGroup, Blend, Range<u32>)]) -> Vec<Drawn<'a>> {
    layers
        .iter()
        .map(|(group, _, range)| Drawn::Picture(group, range.clone()))
        .collect()
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
    let mut render =
        |post: &Post, frames: usize, vertex_buffer: &wgpu::Buffer, steps: &[Drawn]| -> Vec<f32> {
            for _ in 0..frames {
                let mut encoder = device.create_command_encoder(&Default::default());
                compose.run(
                    Inputs {
                        device: &device,
                        queue: &queue,
                        encoder: &mut encoder,
                        vertices: vertex_buffer,
                        layers: steps,
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
    let image = render(&post, 3, &vertex_buffer, &drawn(&alpha));
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
    let image = render(&inverted, 1, &vertex_buffer, &drawn(&alpha));
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
    let image = render(&none, 1, &blends, &drawn(&stack));
    assert!(at(&image, 30, 34) < 0.01, "{}", at(&image, 30, 34));
    assert!(at(&image, 2, 2) > 0.99, "{}", at(&image, 2, 2));
    let first = [(&picture, Blend::Difference, 6..12)];
    let image = render(&none, 1, &blends, &drawn(&first));
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
    let image = render(&none, 1, &blends, &drawn(&after));
    assert!(
        at(&image, 30, 34) > 0.99 && at(&image, 2, 2) > 0.99,
        "{} {}",
        at(&image, 30, 34),
        at(&image, 2, 2)
    );

    // A group is mixed over clear and then drawn like a picture: at full alpha it is the same
    // as its pictures flat, and a blend that reads the light below does not read the light
    // below the group while the group's pictures are mixed.
    let quad = |place: [f32; 4], opacity: f32, shading: f32| -> Vec<Vertex> {
        vertices(&whole_quad())
            .into_iter()
            .map(|v| Vertex {
                place,
                opacity,
                shading,
                ..v
            })
            .collect()
    };
    let mut data = quad(middle, 0.5, 0.0);
    data.extend(quad([0.0, 0.0, 1.0, 1.0], 1.0, 0.0));
    data.extend(quad([0.0, 0.0, 1.0, 1.0], 1.0, Blend::Multiply.shading()));
    data.extend(quad([0.0, 0.0, 1.0, 1.0], 0.5, 0.0));
    let grouped = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: vertex_bytes(&data).len() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&grouped, 0, &vertex_bytes(&data));
    let flat = render(&none, 1, &grouped, &[Drawn::Picture(&picture, 0..6)]);
    let group = [Drawn::Group(vec![(&picture, 0..6)], 6..12)];
    let mixed = render(&none, 1, &grouped, &group);
    for (x, y) in [(32, 32), (2, 2)] {
        let (a, b) = (at(&flat, x, y), at(&mixed, x, y));
        assert!((a - b).abs() < 0.01, "{x},{y}: {a} flat, {b} grouped");
    }
    // Multiplied by white, the grey below stays, even where the group has nothing.
    let grey = [
        Drawn::Picture(&picture, 18..24),
        Drawn::Group(vec![(&picture, 0..6)], 12..18),
    ];
    let image = render(&none, 1, &grouped, &grey);
    let (corner, centre) = (at(&image, 2, 2), at(&image, 32, 32));
    assert!(
        corner > 0.1 && (corner - centre).abs() < 0.02,
        "{corner} {centre}"
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
        let image = render(&none, 1, &buffer, &drawn(&layers));
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
