use super::*;

pub(super) const SHADER: &str = include_str!("shader.wgsl");

/// The layouts of the picture shader's pipelines: the picture and the masks with the effects, and
/// those with the light below as a third group, which the blends that read it have.
pub(super) fn pipeline_layouts(
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
pub(super) fn picture_pipeline(
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
pub(super) fn picture_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
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
pub(super) fn masks_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
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
pub(super) fn whole_quad() -> Quad {
    Quad {
        input: [0.0, 0.0, 1.0, 1.0],
        corners: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        soft: [0.0; 4],
        gamma: 1.0,
    }
}
