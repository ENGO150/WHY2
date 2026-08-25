/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use std::borrow::Cow;

use openh264::formats::YUVSource;

use wgpu::
{
    Adapter,
    BindGroup,
    BindGroupDescriptor,
    BindGroupEntry,
    BindGroupLayout,
    Buffer,
    BufferDescriptor,
    BufferUsages,
    CommandEncoderDescriptor,
    ComputePassDescriptor,
    ComputePipeline,
    ComputePipelineDescriptor,
    Device,
    DeviceDescriptor,
    Instance,
    InstanceDescriptor,
    MapMode,
    PollType,
    PowerPreference,
    Queue,
    RequestAdapterOptions,
    ShaderModuleDescriptor,
    ShaderSource,
};

//CONSTANTS
const WORKGROUP: u32 = 64;

const SHADER: &str = include_str!("rgba_to_i420.wgsl");

//STRUCTS
pub struct I420Frame //I420 PLANES LAID OUT BACK TO BACK, READY FOR THE ENCODER
{
    data: Vec<u8>,
    width: usize,
    height: usize,
}

struct Resources //EVERYTHING THAT DEPENDS ON THE FRAME SIZE
{
    width: u32,
    height: u32,

    source: Buffer,
    target: Buffer,
    staging: Buffer,
    bind_group: BindGroup,

    invocations: u32,
    output_bytes: u64,
}

pub struct GpuConverter
{
    device: Device,
    queue: Queue,
    pipeline: ComputePipeline,
    layout: BindGroupLayout,
    adapter: String,

    resources: Option<Resources>,
    frame: I420Frame,
}

//IMPLEMENTATIONS
impl YUVSource for I420Frame
{
    fn dimensions(&self) -> (usize, usize)
    {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize)
    {
        (self.width, self.width / 2, self.width / 2)
    }

    fn y(&self) -> &[u8]
    {
        &self.data[..self.width * self.height]
    }

    fn u(&self) -> &[u8]
    {
        let base = self.width * self.height;

        &self.data[base..base + base / 4]
    }

    fn v(&self) -> &[u8]
    {
        let base = self.width * self.height;

        &self.data[base + base / 4..base + base / 2]
    }
}

impl GpuConverter
{
    pub fn supports(width: u32, height: u32) -> bool //DIMENSIONS THE SHADER'S WORD PACKING CAN HANDLE
    {
        //FOUR LUMA SAMPLES ARE PACKED PER OUTPUT WORD AND FOUR CHROMA SAMPLES PER CHROMA WORD, SO A
        //ROW MUST BE A WHOLE NUMBER OF WORDS IN BOTH PLANES. ANYTHING ELSE FALLS BACK TO THE CPU.
        width % 8 == 0 && height % 2 == 0 && width > 0 && height > 0
    }

    pub fn new() -> Result<Self, String>
    {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle_from_env());

        let adapter: Adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions
        {
            power_preference: PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        })).map_err(|e| format!("no usable GPU adapter ({e})"))?;

        let name = adapter.get_info().name;

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor
        {
            label: Some("why2 capture converter"),
            ..Default::default()
        })).map_err(|e| format!("requesting a GPU device failed ({e})"))?;

        //A SHADER THAT FAILS TO COMPILE MUST SURFACE AS AN Err RATHER THAN AS A PANIC INSIDE wgpu,
        //BECAUSE THE ONLY SENSIBLE ANSWER TO IT IS TO GO ON USING THE CPU PATH
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let module = device.create_shader_module(ShaderModuleDescriptor
        {
            label: Some("rgba -> i420"),
            source: ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });

        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor
        {
            label: Some("rgba -> i420"),
            layout: None,
            module: &module,
            entry_point: Some("convert"),
            compilation_options: Default::default(),
            cache: None,
        });

        if let Some(error) = pollster::block_on(scope.pop())
        {
            return Err(format!("the conversion shader was rejected ({error})"));
        }

        let layout = pipeline.get_bind_group_layout(0);

        Ok(Self
        {
            device,
            queue,
            pipeline,
            layout,
            adapter: name,
            resources: None,
            frame: I420Frame { data: Vec::new(), width: 0, height: 0 },
        })
    }

    pub fn adapter(&self) -> &str
    {
        &self.adapter
    }

    fn prepare(&mut self, width: u32, height: u32)
    {
        //REALLOCATE ONLY WHEN THE MONITOR RESOLUTION ACTUALLY CHANGED
        if self.resources.as_ref().is_some_and(|r| r.width == width && r.height == height) { return; }

        let pixels = width as u64 * height as u64;

        let y_words = pixels / 4;
        let c_words = pixels / 16; //(width/2 * height/2) / 4

        let source_bytes = pixels * 4;
        let output_bytes = (y_words + 2 * c_words) * 4;

        let source = self.device.create_buffer(&BufferDescriptor
        {
            label: Some("rgba source"),
            size: source_bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let target = self.device.create_buffer(&BufferDescriptor
        {
            label: Some("i420 target"),
            size: output_bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging = self.device.create_buffer(&BufferDescriptor
        {
            label: Some("i420 readback"),
            size: output_bytes,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        //THE UNIFORM IS WRITTEN BY HAND RATHER THAN THROUGH bytemuck - FOUR u32 IS NOT WORTH A
        //DEPENDENCY, AND THE STD140 LAYOUT OF FOUR SCALARS IS JUST THEIR CONCATENATION
        let mut dimensions = Vec::with_capacity(16);
        for value in [width, height, y_words as u32, c_words as u32]
        {
            dimensions.extend_from_slice(&value.to_le_bytes());
        }

        let uniform = self.device.create_buffer(&BufferDescriptor
        {
            label: Some("dimensions"),
            size: 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.queue.write_buffer(&uniform, 0, &dimensions);

        let bind_group = self.device.create_bind_group(&BindGroupDescriptor
        {
            label: Some("rgba -> i420"),
            layout: &self.layout,
            entries:
            &[
                BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: source.as_entire_binding() },
                BindGroupEntry { binding: 2, resource: target.as_entire_binding() },
            ],
        });

        self.resources = Some(Resources
        {
            width,
            height,
            source,
            target,
            staging,
            bind_group,
            invocations: (y_words + c_words) as u32,
            output_bytes,
        });

        self.frame = I420Frame
        {
            data: vec![0; output_bytes as usize],
            width: width as usize,
            height: height as usize,
        };
    }

    pub fn convert(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<&I420Frame, String>
    {
        if !Self::supports(width, height)
        {
            return Err(format!("resolution {width}x{height} is not supported by the GPU converter"));
        }

        let expected = width as usize * height as usize * 4;
        if rgba.len() < expected
        {
            return Err(format!("frame is {} bytes, expected {expected}", rgba.len()));
        }

        self.prepare(width, height);

        let resources = self.resources.as_ref().expect("prepare always installs resources");

        //THE SOURCE IS UPLOADED AS PACKED RGBA WORDS. THE SHADER UNPACKS BYTE 0 AS RED, WHICH IS
        //ONLY THE RIGHT READING ON A LITTLE-ENDIAN HOST - EVERY PLATFORM THIS CLIENT BUILDS FOR IS
        self.queue.write_buffer(&resources.source, 0, &rgba[..expected]);

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor
        {
            label: Some("rgba -> i420"),
        });

        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor
            {
                label: Some("rgba -> i420"),
                timestamp_writes: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &resources.bind_group, &[]);
            pass.dispatch_workgroups(resources.invocations.div_ceil(WORKGROUP), 1, 1);
        }

        encoder.copy_buffer_to_buffer(&resources.target, 0, &resources.staging, 0, resources.output_bytes);

        self.queue.submit(Some(encoder.finish()));

        //READ BACK. THIS IS THE PIPELINE'S ONLY GPU SYNC POINT, AND IT IS WHY THE CONVERTER IS
        //WORTH IT ONLY BECAUSE THE RESULT IS 1.5 BYTES PER PIXEL AGAINST THE 4 THAT WENT UP.
        let slice = resources.staging.slice(..);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        slice.map_async(MapMode::Read, move |result| { ready_tx.send(result).ok(); });

        self.device.poll(PollType::wait_indefinitely())
            .map_err(|e| format!("waiting on the GPU failed ({e})"))?;

        match ready_rx.recv()
        {
            Ok(Ok(())) => {},
            Ok(Err(e)) => return Err(format!("mapping the readback buffer failed ({e})")),
            Err(_) => return Err("the GPU never reported the readback".to_owned()),
        }

        self.frame.data.copy_from_slice(&slice.get_mapped_range());
        resources.staging.unmap();

        Ok(&self.frame)
    }
}
