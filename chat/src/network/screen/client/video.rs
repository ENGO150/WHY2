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

//! # Screen Share Renderer
//!
//! A `wgpu` renderer that draws decoded I420 frames, used by the viewer window
//! ([`VideoSurface`]) and, without a window, by tests ([`YuvRenderer::headless`]).
//!
//! # Pipeline
//! The decoder's three planes are uploaded as `R8Unorm` textures — 1.5 bytes per pixel and no
//! CPU colour conversion — and one fragment shader (`yuv_to_rgba.wgsl`) does the BT.601
//! conversion, the chroma upsampling and the scale to the window in a single draw. The picture
//! is letterboxed, never stretched.
//!
//! The planes are limited range (luma in $16..235$, chroma in $16..240$), so each sample
//! $Y, U, V \in [0, 1]$ is first expanded and then converted, the result clamped to $[0, 1]$:
//!
//! $$ Y^{\prime} = \frac{255}{219}\left(Y - \frac{16}{255}\right) \qquad C\_b = \frac{255}{224}\left(U - \frac{128}{255}\right) \qquad C\_r = \frac{255}{224}\left(V - \frac{128}{255}\right) $$
//!
//! $$ \begin{pmatrix} R \\\\ G \\\\ B \end{pmatrix} = \begin{pmatrix} 1 & 0 & 1.402 \\\\ 1 & -0.344136 & -0.714136 \\\\ 1 & 1.772 & 0 \end{pmatrix} \begin{pmatrix} Y^{\prime} \\\\ C\_b \\\\ C\_r \end{pmatrix} $$
//!
//! # Strides
//! Planes are allocated at the decoder's stride, not its width, and the shader trims the
//! padding. The sampled span reaches the **centre** of the last real texel; spanning to its edge
//! would let the linear sampler mix in half a texel of padding, which shows as a smear down the
//! right-hand column. For a plane $w$ texels wide allocated at stride $S$, the picture's
//! horizontal coordinate $t \in [0, 1]$ samples the plane at
//!
//! $$ u = \frac{0.5}{S} + t \cdot \frac{w - 1}{S} $$
//!
//! which runs from the centre of the first texel to the centre of the last.
//!
//! # Colour
//! The shader's output is already display-referred sRGB (BT.601 video is gamma-encoded), so it is
//! never written through an sRGB view: an sRGB surface is presented through its linear twin. Writing through the sRGB view would encode it twice and wash every
//! shadow out.

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
    SurfaceColorSpace,
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
/// The YUV to RGB shader.
const SHADER: &str = include_str!("yuv_to_rgba.wgsl");

//STRUCTS
/// One texture per I420 plane, allocated at the stride, and the bind group over them.
struct Planes //ONE TEXTURE PER I420 PLANE, AT THE STRIDE
{
    /// Picture width.
    width: u32,
    /// Picture height.
    height: u32,
    /// Luma and chroma strides.
    strides: (u32, u32),

    /// The Y plane.
    luma: Texture,
    /// The U plane.
    chroma_u: Texture,
    /// The V plane.
    chroma_v: Texture,

    /// The bind group over the planes, sampler and geometry.
    bind_group: BindGroup,
}

/// Draws I420 frames into any render target; needs no window.
pub struct YuvRenderer //THE PART THAT NEEDS NO WINDOW
{
    /// The GPU device.
    device: Device,
    /// The device's queue.
    queue: Queue,
    /// The YUV to RGB pipeline.
    pipeline: RenderPipeline,
    /// Layout of the frame bind group.
    layout: BindGroupLayout,
    /// The linear sampler.
    sampler: Sampler,
    /// Uniform buffer with the letterbox scale and the plane spans.
    geometry: Buffer,

    /// The current planes, rebuilt when the frame's size or strides change.
    planes: Option<Planes>,
}

//THE FORMAT THE PICTURE IS WRITTEN THROUGH
/// Returns the format to write the picture through: the surface's format without its sRGB
/// suffix.
fn present_format(surface: TextureFormat) -> TextureFormat
{
    surface.remove_srgb_suffix()
}

/// A window and the renderer that paints it.
pub struct VideoSurface //A WINDOW AND THE RENDERER THAT PAINTS IT
{
    /// The window's surface.
    surface: Surface<'static>,
    /// The surface configuration, kept for resizes.
    configuration: SurfaceConfiguration,
    //THE FORMAT WE WRITE THROUGH
    /// The format frames are written through.
    view_format: TextureFormat,
    /// The renderer.
    renderer: YuvRenderer,
}

//FUNCTIONS
//PRIVATE
/// Creates one plane texture.
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

/// Uploads one plane, `stride` bytes per row.
fn upload_plane(queue: &Queue, texture: &Texture, data: &[u8], stride: u32, height: u32)
{
    //write_texture STAGES, SO THE PITCH NEEDS NO ALIGN
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
    /// Builds the pipeline for a render target format.
    ///
    /// # Returns
    /// `Err` with a description if the shader or pipeline fails validation.
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
            //CLAMP: REPEATING WOULD WRAP THE PADDING IN
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

    /// Creates a renderer on the best available GPU, with no window.
    ///
    /// # Parameters
    /// - `format`: The format of the targets it will draw into.
    ///
    /// # Returns
    /// `Err` with a description if no adapter or device is available.
    pub fn headless(format: TextureFormat) -> Result<Self, String>
    {
        let instance = Instance::new(InstanceDescriptor::new_without_display_handle_from_env());

        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions
        {
            power_preference: PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        })).map_err(|e| format!("no usable GPU adapter ({e})"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor
        {
            label: Some("why2 screen viewer"),
            ..Default::default()
        })).map_err(|e| format!("requesting a GPU device failed ({e})"))?;

        Self::build(device, queue, format)
    }

    /// Rebuilds the planes if the frame's size or strides changed.
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

    /// Uploads one decoded frame.
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

    /// Writes the letterbox scale and plane spans for a target size.
    ///
    /// With frame aspect $a\_f = w/h$ and target aspect $a\_t$, the picture is scaled by
    /// $(a\_f / a\_t,\ 1)$ when the target is wider than the frame and by $(1,\ a\_t / a\_f)$
    /// otherwise.
    fn write_geometry(&self, target: (u32, u32))
    {
        let Some(planes) = &self.planes else { return; };

        //LETTERBOX RATHER THAN STRETCH
        let frame_aspect = planes.width as f32 / planes.height as f32;
        let target_aspect = target.0.max(1) as f32 / target.1.max(1) as f32;

        let scale = if target_aspect > frame_aspect
        {
            [frame_aspect / target_aspect, 1.0]
        } else
        {
            [1.0, target_aspect / frame_aspect]
        };

        //SPAN AND OFFSET REACH THE TEXEL CENTRES
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

    /// Draws the current frame, letterboxed, into `view`; does nothing before the first upload.
    ///
    /// # Parameters
    /// - `view`: The render target.
    /// - `target`: Its size in pixels.
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
                        //THE LETTERBOX BARS
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
    /// Creates a renderer for a window.
    ///
    /// Prefers mailbox presentation, since a live share should never queue frames.
    ///
    /// # Parameters
    /// - `window`: The window to draw into.
    /// - `width`, `height`: Its initial size.
    ///
    /// # Returns
    /// `Err` with a description if the surface, adapter, device or pipeline cannot be created.
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
            apply_limit_buckets: false,
        })).map_err(|e| format!("no usable GPU adapter ({e})"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor
        {
            label: Some("why2 screen viewer"),
            ..Default::default()
        })).map_err(|e| format!("requesting a GPU device failed ({e})"))?;

        let capabilities = surface.get_capabilities(&adapter);

        let format = capabilities.formats[0];

        //WRITE THROUGH A LINEAR VIEW - THE SHADER EMITS sRGB
        let view_format = present_format(format);

        //THE SAME FORMAT BACK IS NOT A VIEW FORMAT
        let view_formats = if view_format == format { vec![] } else { vec![view_format] };

        let configuration = SurfaceConfiguration
        {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            //THE SHARE IS LIVE, SO NEVER QUEUE FRAMES
            present_mode: capabilities.present_modes.iter().copied()
                .find(|mode| *mode == PresentMode::Mailbox)
                .unwrap_or(PresentMode::Fifo),
            alpha_mode: capabilities.alpha_modes[0],
            //Auto KEEPS THE BACKEND OUT OF OUR ENCODING
            color_space: SurfaceColorSpace::Auto,
            view_formats,
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &configuration);

        let renderer = YuvRenderer::build(device, queue, view_format)?;

        Ok(Self { surface, configuration, view_format, renderer })
    }

    /// Uploads one decoded frame.
    pub fn upload(&mut self, frame: &impl YUVSource)
    {
        self.renderer.upload(frame);
    }

    /// Resizes the surface; a zero or unchanged size is ignored.
    pub fn resize(&mut self, width: u32, height: u32)
    {
        if width == 0 || height == 0 { return; }
        if self.configuration.width == width && self.configuration.height == height { return; }

        self.configuration.width = width;
        self.configuration.height = height;

        self.surface.configure(&self.renderer.device, &self.configuration);
    }

    /// Draws and presents the current frame, skipping it if the surface is momentarily unavailable.
    pub fn render(&mut self)
    {
        let frame = match self.surface.get_current_texture()
        {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => frame,

            //TRANSIENT (MINIMISED, BUSY): SKIP THE FRAME
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return,

            //THE SURFACE WENT STALE: RECONFIGURE
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

        self.renderer.queue.present(frame);
    }
}
