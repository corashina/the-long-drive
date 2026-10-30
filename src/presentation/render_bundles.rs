//! Render bundles for draws that repeat each frame. A bundle re-records only when its commands change.
//! Bevy resource IDs are never reused, so equal commands bind the same resources.
use bevy::render::{
    render_phase::{CAPTURE_OFFSETS, PassCommand, TrackedRenderPass},
    render_resource::{BindGroupId, BufferId, IndexFormat, RenderPipelineId, TextureFormat},
    renderer::RenderDevice,
};
use std::ops::Range;

/// The attachments of the passes that execute a bundle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BundleTarget {
    pub color: Option<TextureFormat>,
    pub depth: TextureFormat,
    pub samples: u32,
}

/// A captured command without its resources.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Key {
    Pipeline(RenderPipelineId),
    BindGroup(u32, BindGroupId, [u32; CAPTURE_OFFSETS], usize),
    VertexBuffer(u32, BufferId, u64, u64),
    IndexBuffer(BufferId, u64, u64, IndexFormat),
    Draw(Range<u32>, Range<u32>),
    DrawIndexed(Range<u32>, i32, Range<u32>),
    DrawIndirect(BufferId, u64),
    DrawIndexedIndirect(BufferId, u64),
}
fn key(command: &PassCommand) -> Key {
    match command {
        PassCommand::Pipeline(pipeline) => Key::Pipeline(pipeline.id()),
        PassCommand::BindGroup(index, group, offsets, count) => {
            Key::BindGroup(*index, group.id(), *offsets, *count)
        }
        PassCommand::VertexBuffer(slot, slice) => {
            Key::VertexBuffer(*slot, slice.id(), slice.offset(), slice.size().get())
        }
        PassCommand::IndexBuffer(slice, format) => {
            Key::IndexBuffer(slice.id(), slice.offset(), slice.size().get(), *format)
        }
        PassCommand::Draw(vertices, instances) => Key::Draw(vertices.clone(), instances.clone()),
        PassCommand::DrawIndexed(indices, base, instances) => {
            Key::DrawIndexed(indices.clone(), *base, instances.clone())
        }
        PassCommand::DrawIndirect(buffer, offset) => Key::DrawIndirect(buffer.id(), *offset),
        PassCommand::DrawIndexedIndirect(buffer, offset) => {
            Key::DrawIndexedIndirect(buffer.id(), *offset)
        }
    }
}
fn record<'a>(bundle: &mut wgpu::RenderBundleEncoder<'a>, command: &PassCommand<'a>) {
    match command {
        PassCommand::Pipeline(pipeline) => bundle.set_pipeline(pipeline),
        PassCommand::BindGroup(index, group, offsets, count) => {
            bundle.set_bind_group(*index, &***group, &offsets[..*count]);
        }
        PassCommand::VertexBuffer(slot, slice) => bundle.set_vertex_buffer(*slot, **slice),
        PassCommand::IndexBuffer(slice, format) => bundle.set_index_buffer(**slice, *format),
        PassCommand::Draw(vertices, instances) => bundle.draw(vertices.clone(), instances.clone()),
        PassCommand::DrawIndexed(indices, base, instances) => {
            bundle.draw_indexed(indices.clone(), *base, instances.clone());
        }
        PassCommand::DrawIndirect(buffer, offset) => bundle.draw_indirect(buffer, *offset),
        PassCommand::DrawIndexedIndirect(buffer, offset) => {
            bundle.draw_indexed_indirect(buffer, *offset);
        }
    }
}
/// Keep the allocation of a command list for the next frame. Its commands borrow one frame.
fn reuse<'b>(mut commands: Vec<PassCommand<'_>>) -> Vec<PassCommand<'b>> {
    commands.clear();
    commands.into_iter().map(|_| unreachable!()).collect()
}

/// The bundle of one draw list and the commands it holds.
#[derive(Default)]
pub struct DrawBundle {
    target: Option<BundleTarget>,
    keys: Vec<Key>,
    commands: Vec<PassCommand<'static>>,
    bundle: Option<wgpu::RenderBundle>,
}
impl DrawBundle {
    /// Capture the commands of `draws` and execute them as a bundle.
    /// Fails when `draws` sends a command that a bundle cannot hold. Then nothing is drawn.
    pub fn draw<'w, R>(
        &mut self,
        device: &RenderDevice,
        target: BundleTarget,
        pass: &mut TrackedRenderPass<'w>,
        draws: impl FnOnce(&mut TrackedRenderPass<'w>) -> R,
    ) -> Result<R, String> {
        pass.begin_capture(reuse(std::mem::take(&mut self.commands)));
        let result = draws(pass);
        let capture = pass.end_capture().expect("render bundle capture");
        if let Some(command) = capture.unsupported {
            self.commands = reuse(capture.commands);
            return Err(format!("Render bundle cannot hold {command}"));
        }
        let commands = capture.commands;
        let same = self.target == Some(target)
            && self.keys.len() == commands.len()
            && self
                .keys
                .iter()
                .zip(&commands)
                .all(|(old, new)| *old == key(new));
        if !same {
            self.keys.clear();
            self.keys.extend(commands.iter().map(key));
            self.target = Some(target);
            self.bundle = (!commands.is_empty()).then(|| {
                let colors = [target.color];
                let mut encoder = device.wgpu_device().create_render_bundle_encoder(
                    &wgpu::RenderBundleEncoderDescriptor {
                        label: Some("source_draw_bundle"),
                        color_formats: if target.color.is_some() { &colors } else { &[] },
                        depth_stencil: Some(wgpu::RenderBundleDepthStencil {
                            format: target.depth,
                            depth_read_only: false,
                            // The source passes have no stencil operations.
                            stencil_read_only: true,
                        }),
                        sample_count: target.samples,
                        multiview: None,
                    },
                );
                for command in &commands {
                    record(&mut encoder, command);
                }
                encoder.finish(&wgpu::RenderBundleDescriptor {
                    label: Some("source_draw_bundle"),
                })
            });
        }
        self.commands = reuse(commands);
        if let Some(bundle) = &self.bundle {
            pass.wgpu_pass().execute_bundles(std::iter::once(bundle));
        }
        Ok(result)
    }
}
