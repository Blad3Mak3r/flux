pub(super) fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    clear_to(encoder, view, wgpu::Color::BLACK);
}

pub(super) fn clear_to(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    colour: wgpu::Color,
) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear history"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(colour),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}

/// One triangle over the whole of `target`, drawn by `pipeline` with `group` and the settings at
/// `offset`; `clear` is the colour it starts from, or none to draw over what it holds.
pub(super) fn fullscreen(
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
