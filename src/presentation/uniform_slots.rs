//! Fixed uniform slots in one buffer. Each slot has its own binding range.
//! A frame sets the slots that change, then `upload` sends them in one queue write.
use bevy::render::{
    render_resource::{
        BindingResource, Buffer, BufferBinding, BufferDescriptor, BufferUsages, ShaderType,
        encase::{UniformBuffer, internal::WriteInto},
    },
    renderer::{RenderDevice, RenderQueue},
};
use std::{marker::PhantomData, ops::Range};

pub struct UniformSlots<T> {
    bytes: Vec<u8>,
    scratch: Vec<u8>,
    stride: usize,
    /// Changed slots since the last upload.
    dirty: Range<usize>,
    buffer: Buffer,
    _value: PhantomData<fn(T)>,
}

impl<T: ShaderType + WriteInto> UniformSlots<T> {
    /// `count` zeroed slots.
    pub fn new(device: &RenderDevice, label: &'static str, count: usize) -> Self {
        let size = T::min_size().get() as usize;
        let align = device.limits().min_uniform_buffer_offset_alignment as usize;
        let stride = size.next_multiple_of(align);
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: (stride * count) as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            bytes: vec![0; stride * count],
            scratch: vec![0; size],
            stride,
            dirty: 0..0,
            buffer,
            _value: PhantomData,
        }
    }
    /// Stage a slot. Equal bytes keep the slot clean.
    pub fn set(&mut self, index: usize, value: &T) {
        UniformBuffer::new(&mut self.scratch[..])
            .write(value)
            .expect("uniform slot layout");
        let start = index * self.stride;
        let slot = &mut self.bytes[start..start + self.scratch.len()];
        if *slot == *self.scratch {
            return;
        }
        slot.copy_from_slice(&self.scratch);
        self.dirty = if self.dirty.is_empty() {
            index..index + 1
        } else {
            self.dirty.start.min(index)..self.dirty.end.max(index + 1)
        };
    }
    /// One write covers every changed slot.
    pub fn upload(&mut self, queue: &RenderQueue) {
        if self.dirty.is_empty() {
            return;
        }
        let bytes = self.dirty.start * self.stride..self.dirty.end * self.stride;
        queue.write_buffer(&self.buffer, bytes.start as u64, &self.bytes[bytes]);
        self.dirty = 0..0;
    }
    pub fn binding(&self, index: usize) -> BindingResource<'_> {
        BindingResource::Buffer(BufferBinding {
            buffer: &self.buffer,
            offset: (index * self.stride) as u64,
            size: Some(T::min_size()),
        })
    }
}
