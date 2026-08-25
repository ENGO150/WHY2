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

use std::
{
    borrow::Cow,
    sync::Arc,
};

use openh264::formats::YUVSource;

use winit::window::Window;

use wgpu::
{
    AddressMode,
    BindGroup,
    BindGroupDescriptor,
    BindGroupEntry,
    BindGroupLayout,
    BindingResource,
    Buffer,
    BufferDescriptor,
    BufferUsages,
    Color,
    ColorTargetState,
    CurrentSurfaceTexture,
    ColorWrites,
    CommandEncoderDescriptor,
    Device,
    DeviceDescriptor,
    Extent3d,
    FilterMode,
    FragmentState,
    Instance,
    InstanceDescriptor,
    LoadOp,
    Operations,
    Origin3d,
    PowerPreference,
    PresentMode,
    Queue,
    RenderPassColorAttachment,
    RenderPassDescriptor,
    RenderPipeline,
    RenderPipelineDescriptor,
    RequestAdapterOptions,
    Sampler,
    SamplerDescriptor,
    ShaderModuleDescriptor,
    ShaderSource,
    StoreOp,
    Surface,
    SurfaceConfiguration,
    TexelCopyBufferLayout,
    TexelCopyTextureInfo,
    Texture,
    TextureAspect,
    TextureDescriptor,
    TextureDimension,
    TextureFormat,
    TextureUsages,
    TextureView,
    TextureViewDescriptor,
    VertexState,
};

//CONSTANTS
const SHADER: &str = include_str!("yuv_to_rgba.wgsl");

//STRUCTS
struct Planes //ONE TEXTURE PER I420 PLANE, ALLOCATED AT THE DECODER'S STRIDE
{
    width: u32,
    height: u32,
    strides: (u32, u32),

    luma: Texture,
    chroma_u: Texture,
    chroma_v: Texture,

    bind_group: BindGroup,
}

pub struct YuvRenderer //THE PART THAT NEEDS NO WINDOW
{
    device: Device,
    queue: Queue,
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    sampler: Sampler,
    geometry: Buffer,

    planes: Option<Planes>,
}

//THE FORMAT THE FINISHED PICTURE IS WRITTEN THROUGH. NEVER AN sRGB ONE - SEE `VideoSurface::new`.
fn present_format(surface: TextureFormat) -> TextureFormat
{
    surface.remove_srgb_suffix()
}

pub struct VideoSurface //A WINDOW AND THE RENDERER THAT PAINTS IT
{
    surface: Surface<'static>,
    configuration: SurfaceConfiguration,
    //THE FORMAT WE *WRITE* THROUGH, WHICH IS NOT ALWAYS THE ONE THE SURFACE IS CONFIGURED WITH -
    //SEE THE NOTE ON THE sRGB DOUBLE-ENCODE IN `VideoSurface::new`
    view_format: TextureFormat,
    renderer: YuvRenderer,
}

//FUNCTIONS
//PRIVATE
fn plane_texture(device: &Device, label: &str, width: u32, height: u32) -> Texture
{
    device.create_texture(&TextureDescriptor
    {
        label: Some(label),
        size: Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::R8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn upload_plane(queue: &Queue, texture: &Texture, data: &[u8], stride: u32, height: u32)
{
    //`Queue::write_texture` STAGES INTERNALLY, SO UNLIKE A BUFFER-TO-TEXTURE COPY IT PLACES NO
    //256-BYTE ALIGNMENT DEMAND ON THE ROW PITCH - WHICH IS WHY THE STRIDE CAN GO UP UNTOUCHED
    queue.write_texture
    (
        TexelCopyTextureInfo
        {
            texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        &data[..(stride * height) as usize],
        TexelCopyBufferLayout
        {
            offset: 0,
            bytes_per_row: Some(stride),
            rows_per_image: Some(height),
        },
        Extent3d { width: stride, height, depth_or_array_layers: 1 },
    );
}

//IMPLEMENTATIONS
impl YuvRenderer
{
    fn build(device: Device, queue: Queue, format: TextureFormat) -> Result<Self, String>
    {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let module = device.create_shader_module(ShaderModuleDescriptor
        {
            label: Some("i420 -> rgb"),
            source: ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor
        {
            label: Some("i420 -> rgb"),
            layout: None,
            vertex: VertexState
            {
                module: &module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(FragmentState
            {
                module: &module,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState
                {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        if let Some(error) = pollster::block_on(scope.pop())
        {
            return Err(format!("the presentation shader was rejected ({error})"));
        }

        let layout = pipeline.get_bind_group_layout(0);

        let sampler = device.create_sampler(&SamplerDescriptor
        {
            label: Some("frame"),
            //CLAMP MATTERS: THE PLANES ARE PADDED OUT TO THEIR STRIDE, SO REPEATING WOULD WRAP
            //THE GARBAGE PAST THE END OF A ROW BACK INTO THE PICTURE
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });

        let geometry = device.create_buffer(&BufferDescriptor
        {
            label: Some("geometry"),
            size: 24,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self { device, queue, pipeline, layout, sampler, geometry, planes: None })
    }

    pub fn headless(format: TextureFormat) -> Result<Self, String>
    {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle_from_env());

        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions
        {
            power_preference: PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        })).map_err(|e| format!("no usable GPU adapter ({e})"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor
        {
            label: Some("why2 screen viewer"),
            ..Default::default()
        })).map_err(|e| format!("requesting a GPU device failed ({e})"))?;

        Self::build(device, queue, format)
    }

    fn prepare(&mut self, width: u32, height: u32, strides: (u32, u32))
    {
        if self.planes.as_ref().is_some_and(|planes|
            planes.width == width && planes.height == height && planes.strides == strides)
        {
            return;
        }

        let luma = plane_texture(&self.device, "luma", strides.0, height);
        let chroma_u = plane_texture(&self.device, "chroma u", strides.1, height / 2);
        let chroma_v = plane_texture(&self.device, "chroma v", strides.1, height / 2);

        let views: Vec<TextureView> = [&luma, &chroma_u, &chroma_v].iter()
            .map(|texture| texture.create_view(&TextureViewDescriptor::default()))
            .collect();

        let bind_group = self.device.create_bind_group(&BindGroupDescriptor
        {
            label: Some("frame"),
            layout: &self.layout,
            entries:
            &[
                BindGroupEntry { binding: 0, resource: self.geometry.as_entire_binding() },
                BindGroupEntry { binding: 1, resource: BindingResource::TextureView(&views[0]) },
                BindGroupEntry { binding: 2, resource: BindingResource::TextureView(&views[1]) },
                BindGroupEntry { binding: 3, resource: BindingResource::TextureView(&views[2]) },
                BindGroupEntry { binding: 4, resource: BindingResource::Sampler(&self.sampler) },
            ],
        });

        self.planes = Some(Planes { width, height, strides, luma, chroma_u, chroma_v, bind_group });
    }

    pub fn upload(&mut self, frame: &impl YUVSource) //HAND ONE DECODED FRAME TO THE GPU
    {
        let (width, height) = frame.dimensions();
        let (stride_y, stride_u, _) = frame.strides();

        let (width, height) = (width as u32, height as u32);
        let strides = (stride_y as u32, stride_u as u32);

        if width == 0 || height < 2 { return; }

        self.prepare(width, height, strides);

        let Some(planes) = &self.planes else { return; };

        upload_plane(&self.queue, &planes.luma, frame.y(), strides.0, height);
        upload_plane(&self.queue, &planes.chroma_u, frame.u(), strides.1, height / 2);
        upload_plane(&self.queue, &planes.chroma_v, frame.v(), strides.1, height / 2);
    }

    fn write_geometry(&self, target: (u32, u32))
    {
        let Some(planes) = &self.planes else { return; };

        //LETTERBOX RATHER THAN STRETCH - THE `pixels` PATH USED ScalingMode::Fill, WHICH SILENTLY
        //DISTORTED ANY SHARE WHOSE ASPECT DID NOT MATCH THE WINDOW
        let frame_aspect = planes.width as f32 / planes.height as f32;
        let target_aspect = target.0.max(1) as f32 / target.1.max(1) as f32;

        let scale = if target_aspect > frame_aspect
        {
            [frame_aspect / target_aspect, 1.0]
        } else
        {
            [1.0, target_aspect / frame_aspect]
        };

        //SPAN REACHES THE CENTRE OF THE LAST REAL TEXEL; OFFSET STARTS AT THE CENTRE OF THE FIRST
        let span = |real: u32, stride: u32| -> [f32; 2]
        {
            [(real.saturating_sub(1)) as f32 / stride as f32, 0.5 / stride as f32]
        };

        let luma = span(planes.width, planes.strides.0);
        let chroma = span(planes.width / 2, planes.strides.1);

        let mut data = Vec::with_capacity(24);
        for value in [scale[0], scale[1], luma[0], luma[1], chroma[0], chroma[1]]
        {
            data.extend_from_slice(&value.to_le_bytes());
        }

        self.queue.write_buffer(&self.geometry, 0, &data);
    }

    pub fn draw(&self, view: &TextureView, target: (u32, u32))
    {
        let Some(planes) = &self.planes else { return; };

        self.write_geometry(target);

        let mut encoder = self.device.create_command_encoder(&CommandEncoderDescriptor
        {
            label: Some("present"),
        });

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor
            {
                label: Some("present"),
                color_attachments: &[Some(RenderPassColorAttachment
                {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations
                    {
                        //THE LETTERBOX BARS ARE THIS CLEAR, NOT A STRETCHED PICTURE
                        load: LoadOp::Clear(Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &planes.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
    }
}

impl VideoSurface
{
    pub fn new(window: Arc<Window>, width: u32, height: u32) -> Result<Self, String>
    {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle_from_env());

        let surface = instance.create_surface(window)
            .map_err(|e| format!("creating the window surface failed ({e})"))?;

        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions
        {
            power_preference: PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        })).map_err(|e| format!("no usable GPU adapter ({e})"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor
        {
            label: Some("why2 screen viewer"),
            ..Default::default()
        })).map_err(|e| format!("requesting a GPU device failed ({e})"))?;

        let capabilities = surface.get_capabilities(&adapter);

        let format = capabilities.formats[0];

        //THE FRAGMENT SHADER ALREADY EMITS DISPLAY-REFERRED sRGB: BT.601 OUTPUT IS GAMMA-ENCODED
        //VIDEO, NOT LINEAR LIGHT. WRITING IT THROUGH AN *sRGB* VIEW MAKES THE GPU ENCODE IT A
        //SECOND TIME, WHICH LIFTS EVERY MIDTONE AND DRAINS THE WHOLE PICTURE GREY - THE EXACT
        //WASHED-OUT LOOK, NOT A SUBTLE SHIFT. THE PICTURE THEREFORE GOES OUT THROUGH THE LINEAR
        //VIEW OF WHATEVER THE SURFACE PREFERS, WHICH IS ALSO WHY THE HEADLESS TESTS (Rgba8Unorm,
        //NON-sRGB BY CONSTRUCTION) AGREED WITH THE CPU REFERENCE WHILE A REAL WINDOW DID NOT.
        let view_format = present_format(format);

        //ASKING FOR THE SAME FORMAT BACK IS NOT A VIEW FORMAT, IT IS THE DEFAULT
        let view_formats = if view_format == format { vec![] } else { vec![view_format] };

        let configuration = SurfaceConfiguration
        {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            //THE SHARE IS LIVE: A LATE FRAME IS WORSE THAN A DROPPED ONE, AND Fifo WOULD QUEUE THEM
            present_mode: capabilities.present_modes.iter().copied()
                .find(|mode| *mode == PresentMode::Mailbox)
                .unwrap_or(PresentMode::Fifo),
            alpha_mode: capabilities.alpha_modes[0],
            view_formats,
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &configuration);

        let renderer = YuvRenderer::build(device, queue, view_format)?;

        Ok(Self { surface, configuration, view_format, renderer })
    }

    pub fn upload(&mut self, frame: &impl YUVSource)
    {
        self.renderer.upload(frame);
    }

    pub fn resize(&mut self, width: u32, height: u32)
    {
        if width == 0 || height == 0 { return; }
        if self.configuration.width == width && self.configuration.height == height { return; }

        self.configuration.width = width;
        self.configuration.height = height;

        self.surface.configure(&self.renderer.device, &self.configuration);
    }

    pub fn render(&mut self)
    {
        let frame = match self.surface.get_current_texture()
        {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => frame,

            //TRANSIENT: THE WINDOW IS MINIMISED OR THE COMPOSITOR IS BUSY. SKIP THE FRAME - A LIVE
            //SHARE HAS A NEWER ONE COMING ANYWAY
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return,

            //THE SURFACE ITSELF WENT STALE (MOVED BETWEEN MONITORS, COMPOSITOR RESTARTED) -
            //RECONFIGURE AND LET THE NEXT REDRAW HAVE IT
            _ =>
            {
                self.surface.configure(&self.renderer.device, &self.configuration);
                return;
            },
        };

        let view = frame.texture.create_view(&TextureViewDescriptor
        {
            format: Some(self.view_format),
            ..Default::default()
        });

        self.renderer.draw(&view, (self.configuration.width, self.configuration.height));

        frame.present();
    }
}

//TESTS
#[cfg(test)]
mod test
{
    use super::*;

    //THE VIEWER RENDERS BT.601 OUTPUT, WHICH IS GAMMA-ENCODED VIDEO AND NOT LINEAR LIGHT. AN sRGB
    //TARGET ENCODES IT AGAIN ON WRITE AND THE WHOLE PICTURE GOES GREY. THE HEADLESS COLOUR TESTS
    //BELOW CANNOT CATCH THAT - THEY RENDER TO Rgba8Unorm, WHICH IS NON-sRGB BY CONSTRUCTION, SO
    //THEY PASSED WHILE A REAL WINDOW WAS VISIBLY WASHED OUT. THIS IS THE CHECK THAT WOULD HAVE.
    #[test]
    fn the_picture_is_never_written_through_an_srgb_view()
    {
        let offered =
        [
            TextureFormat::Bgra8UnormSrgb,
            TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Bgra8Unorm,
            TextureFormat::Rgba8Unorm,
        ];

        for format in offered
        {
            let chosen = present_format(format);

            assert!(!chosen.is_srgb(), "{format:?} would be presented through {chosen:?}, which double-encodes");

            //AND IT HAS TO STAY A VIEW OF THE *SAME* TEXTURE, NOT A DIFFERENT ONE
            assert_eq!(chosen.remove_srgb_suffix(), format.remove_srgb_suffix());
        }
    }

    //A SOLID-COLOUR I420 FRAME. STRIDES ARE DELIBERATELY WIDER THAN THE PICTURE SO THE PADDING
    //TRIM IN THE SHADER IS EXERCISED RATHER THAN ASSUMED.
    struct Solid
    {
        width: usize,
        height: usize,
        stride_y: usize,
        stride_c: usize,
        y: Vec<u8>,
        u: Vec<u8>,
        v: Vec<u8>,
    }

    impl Solid
    {
        fn new(width: usize, height: usize, y: u8, u: u8, v: u8) -> Self
        {
            let stride_y = width + 16;
            let stride_c = width / 2 + 8;

            Self
            {
                width,
                height,
                stride_y,
                stride_c,

                //THE PADDING IS FILLED WITH A VALUE THAT WOULD BE OBVIOUS IF IT LEAKED INTO VIEW
                y: [vec![y; width], vec![255; 16]].concat().repeat(height),
                u: [vec![u; width / 2], vec![0; 8]].concat().repeat(height / 2),
                v: [vec![v; width / 2], vec![0; 8]].concat().repeat(height / 2),
            }
        }
    }

    impl YUVSource for Solid
    {
        fn dimensions(&self) -> (usize, usize) { (self.width, self.height) }
        fn strides(&self) -> (usize, usize, usize) { (self.stride_y, self.stride_c, self.stride_c) }
        fn y(&self) -> &[u8] { &self.y }
        fn u(&self) -> &[u8] { &self.u }
        fn v(&self) -> &[u8] { &self.v }
    }

    fn render(renderer: &mut YuvRenderer, frame: &Solid) -> Vec<u8> //RENDER OFFSCREEN AND READ BACK
    {
        let (width, height) = (frame.width as u32, frame.height as u32);

        let target = renderer.device.create_texture(&TextureDescriptor
        {
            label: Some("offscreen"),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        renderer.upload(frame);
        renderer.draw(&target.create_view(&TextureViewDescriptor::default()), (width, height));

        //BUFFER-TO-TEXTURE COPIES *DO* DEMAND A 256-BYTE ROW PITCH, UNLIKE `write_texture`
        let padded = (width * 4).div_ceil(256) * 256;

        let readback = renderer.device.create_buffer(&BufferDescriptor
        {
            label: Some("readback"),
            size: (padded * height) as u64,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = renderer.device.create_command_encoder(&CommandEncoderDescriptor { label: None });

        encoder.copy_texture_to_buffer
        (
            TexelCopyTextureInfo
            {
                texture: &target,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo
            {
                buffer: &readback,
                layout: TexelCopyBufferLayout
                {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            Extent3d { width, height, depth_or_array_layers: 1 },
        );

        renderer.queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();

        slice.map_async(wgpu::MapMode::Read, move |result| { tx.send(result).ok(); });
        renderer.device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU must finish");

        rx.recv().expect("the readback must report").expect("the readback must succeed");

        let mapped = slice.get_mapped_range().to_vec();
        readback.unmap();

        mapped.chunks(padded as usize)
            .flat_map(|row| row[..(width * 4) as usize].to_vec())
            .collect()
    }

    fn pixel(image: &[u8], width: usize, x: usize, y: usize) -> (u8, u8, u8)
    {
        let base = (y * width + x) * 4;

        (image[base], image[base + 1], image[base + 2])
    }

    fn close(actual: (u8, u8, u8), expected: (u8, u8, u8), tolerance: u8) -> bool
    {
        actual.0.abs_diff(expected.0) <= tolerance
            && actual.1.abs_diff(expected.1) <= tolerance
            && actual.2.abs_diff(expected.2) <= tolerance
    }

    #[test]
    fn yuv_is_converted_to_the_expected_colours()
    {
        //NO GPU HERE IS NOT A FAILURE - IT IS THE CASE THE CPU VIEWER FALLBACK EXISTS FOR
        let Ok(mut renderer) = YuvRenderer::headless(TextureFormat::Rgba8Unorm) else { return; };

        let (width, height) = (64usize, 32usize);

        //BT.601 LIMITED RANGE: THESE ARE THE CANONICAL ENDPOINTS AND PRIMARIES
        let cases =
        [
            ("black", (16u8, 128u8, 128u8), (0u8, 0u8, 0u8)),
            ("white", (235, 128, 128), (255, 255, 255)),
            ("red", (81, 90, 240), (255, 0, 0)),
            ("green", (145, 54, 34), (0, 255, 0)),
            ("blue", (41, 240, 110), (0, 0, 255)),
        ];

        for (name, (y, u, v), expected) in cases
        {
            let frame = Solid::new(width, height, y, u, v);
            let image = render(&mut renderer, &frame);

            //SAMPLED AWAY FROM EVERY EDGE, SO A WRONG LETTERBOX WOULD SHOW UP AS THE CLEAR COLOUR
            let centre = pixel(&image, width, width / 2, height / 2);

            assert!(close(centre, expected, 4), "{name}: got {centre:?}, expected {expected:?}");
        }
    }

    #[test]
    fn row_padding_never_reaches_the_picture()
    {
        let Ok(mut renderer) = YuvRenderer::headless(TextureFormat::Rgba8Unorm) else { return; };

        let (width, height) = (64usize, 32usize);

        //MID GREY, WITH THE LUMA PADDING SET TO 255 BY `Solid::new`. IF THE STRIDE TRIM WERE WRONG
        //THE RIGHT-HAND COLUMN WOULD BE DRAGGED TOWARDS WHITE.
        let frame = Solid::new(width, height, 126, 128, 128);
        let image = render(&mut renderer, &frame);

        let middle = pixel(&image, width, width / 2, height / 2);
        let rightmost = pixel(&image, width, width - 1, height / 2);

        assert!(close(rightmost, middle, 4),
            "right edge {rightmost:?} drifted from the middle {middle:?} - stride padding leaked in");
    }
}
