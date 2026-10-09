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
    ffi::{ CStr, c_void },
    mem,
    ptr,
    slice,
};

use ash::
{
    Device,
    Entry,
    Instance,
    khr,
    vk::
    {
        self,
        native,
    },
};

use crate::network::screen::client::encoder::
{
    self,
    Backend,
    Budget,
    Planes,
    Settings,
};

const H264_ENCODE_STD: &CStr = c"VK_STD_vulkan_video_codec_h264_encode";
const H264_ENCODE_STD_VERSION: u32 = 1 << 22; //1.0.0
const DPB_SLOTS: u32 = 2;                     //CURRENT PICTURE AND REFERENCE
const NO_REFERENCE: u8 = 0xff;
const MAX_FRAME_NUM: u32 = 256;               //log2_max_frame_num_minus4 = 4
const BITSTREAM_SLACK: u64 = 1 << 16;
const STATUS_COMPLETE: i32 = 1;
const FENCE_TIMEOUT: u64 = 1_000_000_000;     //ONE SECOND

//LEVEL, MAX MBs PER FRAME, PER SECOND
const LEVELS: [(native::StdVideoH264LevelIdc, u64, u64); 8] =
[
    (native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_1, 8_192, 245_760),
    (native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_2, 8_704, 522_240),
    (native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_0, 22_080, 589_824),
    (native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_1, 36_864, 983_040),
    (native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_2, 36_864, 2_073_600),
    (native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_6_0, 139_264, 4_177_920),
    (native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_6_1, 139_264, 8_355_840),
    (native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_6_2, 139_264, 16_711_680),
];

//STRUCTS
struct Profile //CONSTRAINED BASELINE, BOXED
{
    usage: vk::VideoEncodeUsageInfoKHR<'static>,
    h264: vk::VideoEncodeH264ProfileInfoKHR<'static>,
    info: vk::VideoProfileInfoKHR<'static>,
}

impl Profile
{
    fn new() -> Box<Self>
    {
        let mut profile = Box::new(Self
        {
            usage: vk::VideoEncodeUsageInfoKHR::default()
                .video_usage_hints(vk::VideoEncodeUsageFlagsKHR::STREAMING)
                .video_content_hints(vk::VideoEncodeContentFlagsKHR::DESKTOP)
                .tuning_mode(vk::VideoEncodeTuningModeKHR::LOW_LATENCY),

            h264: vk::VideoEncodeH264ProfileInfoKHR::default()
                .std_profile_idc(native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_BASELINE),

            info: vk::VideoProfileInfoKHR::default()
                .video_codec_operation(vk::VideoCodecOperationFlagsKHR::ENCODE_H264)
                .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::TYPE_420)
                .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
                .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8),
        });

        profile.h264.p_next = &profile.usage as *const _ as *const c_void;
        profile.info.p_next = &profile.h264 as *const _ as *const c_void;

        profile
    }

    fn list(&self) -> vk::VideoProfileListInfoKHR<'_>
    {
        vk::VideoProfileListInfoKHR::default().profiles(slice::from_ref(&self.info))
    }
}

struct Candidate //A USABLE GPU
{
    physical: vk::PhysicalDevice,
    encode_family: u32,
    upload_family: u32,
    status: bool,
    size_alignment: u64,
    granularity: vk::Extent2D,
    min_extent: vk::Extent2D,
    max_extent: vk::Extent2D,
    rate_modes: vk::VideoEncodeRateControlModeFlagsKHR,
    max_bitrate: u64,
    feedback: vk::VideoEncodeFeedbackFlagsKHR,
    max_level: native::StdVideoH264LevelIdc,
}

#[derive(Default)]
struct Picture //IMAGE, MEMORY, VIEW
{
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
}

struct Mapped //PERSISTENTLY MAPPED BUFFER
{
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    pointer: *mut u8,
    size: u64,
    coherent: bool,
}

impl Default for Mapped
{
    fn default() -> Self
    {
        Self { buffer: vk::Buffer::null(), memory: vk::DeviceMemory::null(), pointer: ptr::null_mut(), size: 0, coherent: true }
    }
}

#[derive(Clone, Copy)]
struct Reference //THE LAST PICTURE
{
    slot: u32,
    frame_num: u32,
    poc: i32,
    idr: bool,
}

pub struct VulkanEncoder //H.264 THROUGH VULKAN VIDEO
{
    entry: Entry,
    instance: Instance,
    device: Device,
    video: khr::video_queue::Device,
    video_encode: khr::video_encode_queue::Device,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    profile: Box<Profile>,
    upload_queue: vk::Queue,
    encode_queue: vk::Queue,
    families: Vec<u32>,
    upload_pool: vk::CommandPool,
    encode_pool: vk::CommandPool,
    upload_commands: vk::CommandBuffer,
    encode_commands: vk::CommandBuffer,
    uploaded: vk::Semaphore,
    encoded: vk::Fence,
    session: vk::VideoSessionKHR,
    session_memory: Vec<vk::DeviceMemory>,
    parameters: vk::VideoSessionParametersKHR,
    source: Picture,
    dpb: Picture,
    staging: Mapped,
    bitstream: Mapped,
    feedback: vk::QueryPool,
    status: bool,
    headers: Vec<u8>,
    coded: vk::Extent2D,
    rate_mode: vk::VideoEncodeRateControlModeFlagsKHR,
    bitrate: u64,
    applied: u64, //WHAT THE SESSION HOLDS
    max_bitrate: u64,
    fps: u32,
    period: u32,
    reset: bool,
    dpb_ready: bool,
    reference: Option<Reference>,
    since_idr: u32,
    idr_pic_id: u16,
    budget: Budget,
}

impl VulkanEncoder
{
    pub fn new(settings: Settings) -> Result<Self, String>
    {
        //SAFETY: SYSTEM VULKAN LOADER
        let entry = unsafe { Entry::load() }.map_err(|error| error.to_string())?;

        let application = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_3);

        //SAFETY: PLAIN INSTANCE
        let instance = unsafe { entry.create_instance(&vk::InstanceCreateInfo::default().application_info(&application), None) }
            .map_err(|error| error.to_string())?;

        let profile = Profile::new();

        let candidate = match select(&entry, &instance, &profile)
        {
            Ok(candidate) => candidate,

            Err(error) =>
            {
                //SAFETY: UNUSED INSTANCE
                unsafe { instance.destroy_instance(None) };
                return Err(error);
            },
        };

        let device = match create_device(&instance, &candidate)
        {
            Ok(device) => device,

            Err(error) =>
            {
                //SAFETY: UNUSED INSTANCE
                unsafe { instance.destroy_instance(None) };
                return Err(error);
            },
        };

        //SAFETY: OUR PHYSICAL DEVICE
        let memory_properties = unsafe { instance.get_physical_device_memory_properties(candidate.physical) };

        let families = if candidate.upload_family == candidate.encode_family
        {
            vec![candidate.encode_family]
        } else
        {
            vec![candidate.upload_family, candidate.encode_family]
        };

        //SAFETY: ONE QUEUE EACH
        let (upload_queue, encode_queue) = unsafe
        {
            (device.get_device_queue(candidate.upload_family, 0), device.get_device_queue(candidate.encode_family, 0))
        };

        let mut encoder = Self
        {
            video: khr::video_queue::Device::new(&instance, &device),
            video_encode: khr::video_encode_queue::Device::new(&instance, &device),
            entry,
            instance,
            device,
            memory_properties,
            profile,
            upload_queue,
            encode_queue,
            families,
            upload_pool: vk::CommandPool::null(),
            encode_pool: vk::CommandPool::null(),
            upload_commands: vk::CommandBuffer::null(),
            encode_commands: vk::CommandBuffer::null(),
            uploaded: vk::Semaphore::null(),
            encoded: vk::Fence::null(),
            session: vk::VideoSessionKHR::null(),
            session_memory: Vec::new(),
            parameters: vk::VideoSessionParametersKHR::null(),
            source: Picture::default(),
            dpb: Picture::default(),
            staging: Mapped::default(),
            bitstream: Mapped::default(),
            feedback: vk::QueryPool::null(),
            status: candidate.status,
            headers: Vec::new(),
            coded: vk::Extent2D { width: settings.width.next_multiple_of(16), height: settings.height.next_multiple_of(16) },
            rate_mode: vk::VideoEncodeRateControlModeFlagsKHR::DEFAULT,
            bitrate: u64::from(settings.bitrate).min(candidate.max_bitrate.max(1)),
            applied: 0,
            max_bitrate: candidate.max_bitrate.max(1),
            fps: settings.fps.max(1),
            period: settings.keyframe_interval.clamp(1, MAX_FRAME_NUM),
            reset: false,
            dpb_ready: false,
            reference: None,
            since_idr: 0,
            idr_pic_id: 0,
            budget: Budget::new(settings.bitrate),
        };

        //Drop CLEANS UP A FAILURE
        encoder.build(settings, &candidate)?;

        Ok(encoder)
    }

    fn build(&mut self, settings: Settings, candidate: &Candidate) -> Result<(), String>
    {
        let coded = self.coded;

        if coded.width < candidate.min_extent.width || coded.height < candidate.min_extent.height
            || coded.width > candidate.max_extent.width || coded.height > candidate.max_extent.height
        {
            return Err(format!("{}x{} is outside what the encoder takes", settings.width, settings.height));
        }

        let mut bits = vk::VideoEncodeFeedbackFlagsKHR::BITSTREAM_BUFFER_OFFSET;
        bits |= vk::VideoEncodeFeedbackFlagsKHR::BITSTREAM_BYTES_WRITTEN;

        if !candidate.feedback.contains(bits) { return Err("the encoder reports no bitstream size".to_owned()); }

        self.rate_mode = [vk::VideoEncodeRateControlModeFlagsKHR::VBR, vk::VideoEncodeRateControlModeFlagsKHR::CBR]
            .into_iter()
            .find(|mode| candidate.rate_modes.contains(*mode))
            .unwrap_or(vk::VideoEncodeRateControlModeFlagsKHR::DEFAULT);

        let level = level(coded, self.fps, candidate.max_level)?;

        //IMAGES AT THE ENCODER'S GRANULARITY
        let allocated = vk::Extent2D
        {
            width: coded.width.next_multiple_of(candidate.granularity.width.max(1)),
            height: coded.height.next_multiple_of(candidate.granularity.height.max(1)),
        };

        self.create_commands(candidate)?;
        self.create_session(candidate.physical, candidate.encode_family, allocated)?;
        self.create_parameters(settings, level)?;
        self.create_pictures(candidate.physical, candidate.encode_family, allocated)?;
        self.create_buffers(candidate)?;
        self.create_feedback()?;

        Ok(())
    }

    fn create_commands(&mut self, candidate: &Candidate) -> Result<(), String>
    {
        let device = &self.device;

        //SAFETY: FREED IN Drop
        unsafe
        {
            let flags = vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER;

            self.upload_pool = device.create_command_pool(&vk::CommandPoolCreateInfo::default().flags(flags).queue_family_index(candidate.upload_family), None)
                .map_err(|error| error.to_string())?;

            self.encode_pool = device.create_command_pool(&vk::CommandPoolCreateInfo::default().flags(flags).queue_family_index(candidate.encode_family), None)
                .map_err(|error| error.to_string())?;

            let allocate = |pool| vk::CommandBufferAllocateInfo::default().command_pool(pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);

            self.upload_commands = device.allocate_command_buffers(&allocate(self.upload_pool)).map_err(|error| error.to_string())?[0];
            self.encode_commands = device.allocate_command_buffers(&allocate(self.encode_pool)).map_err(|error| error.to_string())?[0];

            self.uploaded = device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None).map_err(|error| error.to_string())?;
            self.encoded = device.create_fence(&vk::FenceCreateInfo::default(), None).map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    fn create_session(&mut self, physical: vk::PhysicalDevice, family: u32, allocated: vk::Extent2D) -> Result<(), String>
    {
        let format = self.video_format(physical, vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR)?.format;
        let reference_format = self.video_format(physical, vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR)?.format;

        let header = vk::ExtensionProperties::default()
            .extension_name(H264_ENCODE_STD)
            .map_err(|error| error.to_string())?
            .spec_version(H264_ENCODE_STD_VERSION);

        let info = vk::VideoSessionCreateInfoKHR::default()
            .queue_family_index(family)
            .video_profile(&self.profile.info)
            .picture_format(format)
            .max_coded_extent(allocated)
            .reference_picture_format(reference_format)
            .max_dpb_slots(DPB_SLOTS)
            .max_active_reference_pictures(1)
            .std_header_version(&header);

        let handle = self.device.handle();
        let fp = self.video.fp();

        //SAFETY: info OUTLIVES THE CALLS
        unsafe
        {
            (fp.create_video_session_khr)(handle, &info, ptr::null(), &mut self.session).result().map_err(|error| error.to_string())?;

            let mut count = 0;
            (fp.get_video_session_memory_requirements_khr)(handle, self.session, &mut count, ptr::null_mut()).result().map_err(|error| error.to_string())?;

            let mut requirements = vec![vk::VideoSessionMemoryRequirementsKHR::default(); count as usize];
            (fp.get_video_session_memory_requirements_khr)(handle, self.session, &mut count, requirements.as_mut_ptr()).result().map_err(|error| error.to_string())?;

            let mut binds = Vec::with_capacity(requirements.len());

            for requirement in &requirements
            {
                let needs = requirement.memory_requirements;

                let memory = self.allocate(needs, vk::MemoryPropertyFlags::empty(), vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
                self.session_memory.push(memory);

                binds.push(vk::BindVideoSessionMemoryInfoKHR::default()
                    .memory_bind_index(requirement.memory_bind_index)
                    .memory(memory)
                    .memory_size(needs.size));
            }

            (fp.bind_video_session_memory_khr)(handle, self.session, binds.len() as u32, binds.as_ptr()).result().map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    fn create_parameters(&mut self, settings: Settings, level: native::StdVideoH264LevelIdc) -> Result<(), String>
    {
        let coded = self.coded;

        //SAFETY: PLAIN C STRUCTS
        let (mut sps, mut vui, hrd, scaling, mut pps) = unsafe
        {
            (
                mem::zeroed::<native::StdVideoH264SequenceParameterSet>(),
                mem::zeroed::<native::StdVideoH264SequenceParameterSetVui>(),
                mem::zeroed::<native::StdVideoH264HrdParameters>(),
                mem::zeroed::<native::StdVideoH264ScalingLists>(),
                mem::zeroed::<native::StdVideoH264PictureParameterSet>(),
            )
        };

        //NO REORDERING
        vui.flags.set_bitstream_restriction_flag(1);
        vui.aspect_ratio_idc = native::StdVideoH264AspectRatioIdc_STD_VIDEO_H264_ASPECT_RATIO_IDC_UNSPECIFIED;
        vui.video_format = 5;
        vui.max_num_reorder_frames = 0;
        vui.max_dec_frame_buffering = 1;
        vui.pHrdParameters = &hrd;

        let crop_right = (coded.width - settings.width) / 2;
        let crop_bottom = (coded.height - settings.height) / 2;

        sps.flags.set_constraint_set1_flag(1);
        sps.flags.set_frame_mbs_only_flag(1);
        sps.flags.set_direct_8x8_inference_flag(1);
        sps.flags.set_frame_cropping_flag(u32::from(crop_right > 0 || crop_bottom > 0));
        sps.flags.set_vui_parameters_present_flag(1);
        sps.profile_idc = native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_BASELINE;
        sps.level_idc = level;
        sps.chroma_format_idc = native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_420;
        sps.log2_max_frame_num_minus4 = (MAX_FRAME_NUM.ilog2() - 4) as u8;
        sps.pic_order_cnt_type = native::StdVideoH264PocType_STD_VIDEO_H264_POC_TYPE_2;
        sps.max_num_ref_frames = 1;
        sps.pic_width_in_mbs_minus1 = coded.width / 16 - 1;
        sps.pic_height_in_map_units_minus1 = coded.height / 16 - 1;
        sps.frame_crop_right_offset = crop_right;
        sps.frame_crop_bottom_offset = crop_bottom;
        sps.pScalingLists = &scaling;
        sps.pSequenceParameterSetVui = &vui;

        //CAVLC, NO 8x8, NO WEIGHTS
        pps.flags.set_deblocking_filter_control_present_flag(1);
        pps.pScalingLists = &scaling;

        let add = vk::VideoEncodeH264SessionParametersAddInfoKHR::default()
            .std_sp_ss(slice::from_ref(&sps))
            .std_pp_ss(slice::from_ref(&pps));

        let mut h264 = vk::VideoEncodeH264SessionParametersCreateInfoKHR::default()
            .max_std_sps_count(1)
            .max_std_pps_count(1)
            .parameters_add_info(&add);

        let info = vk::VideoSessionParametersCreateInfoKHR::default()
            .video_session(self.session)
            .push_next(&mut h264);

        let handle = self.device.handle();

        //SAFETY: info OUTLIVES THE CALL
        unsafe
        {
            (self.video.fp().create_video_session_parameters_khr)(handle, &info, ptr::null(), &mut self.parameters)
                .result()
                .map_err(|error| error.to_string())?;
        }

        let mut h264_get = vk::VideoEncodeH264SessionParametersGetInfoKHR::default()
            .write_std_sps(true)
            .write_std_pps(true);

        let get = vk::VideoEncodeSessionParametersGetInfoKHR::default()
            .video_session_parameters(self.parameters)
            .push_next(&mut h264_get);

        let fp = self.video_encode.fp();
        let mut size = 0usize;

        //SAFETY: headers IS size BYTES
        unsafe
        {
            (fp.get_encoded_video_session_parameters_khr)(handle, &get, ptr::null_mut(), &mut size, ptr::null_mut())
                .result()
                .map_err(|error| error.to_string())?;

            self.headers = vec![0; size];

            (fp.get_encoded_video_session_parameters_khr)(handle, &get, ptr::null_mut(), &mut size, self.headers.as_mut_ptr() as *mut c_void)
                .result()
                .map_err(|error| error.to_string())?;
        }

        self.headers.truncate(size);

        //THE DRIVER WROTE SPS AND PPS
        let types: Vec<u8> = encoder::nal_units(&self.headers).filter_map(|unit| unit.first().map(|header| header & 0x1f)).collect();

        if !types.contains(&7) || !types.contains(&8) { return Err("the driver wrote no parameter sets".to_owned()); }

        Ok(())
    }

    fn create_pictures(&mut self, physical: vk::PhysicalDevice, family: u32, allocated: vk::Extent2D) -> Result<(), String>
    {
        let source_format = self.video_format(physical, vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR | vk::ImageUsageFlags::TRANSFER_DST)?;
        let dpb_format = self.video_format(physical, vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR)?;

        let sharing = if self.families.len() > 1 { vk::SharingMode::CONCURRENT } else { vk::SharingMode::EXCLUSIVE };
        let families = self.families.clone();

        self.source = self.create_picture
        (
            &source_format,
            vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR | vk::ImageUsageFlags::TRANSFER_DST,
            allocated,
            1,
            sharing,
            &families,
        )?;

        self.dpb = self.create_picture
        (
            &dpb_format,
            vk::ImageUsageFlags::VIDEO_ENCODE_DPB_KHR,
            allocated,
            DPB_SLOTS,
            vk::SharingMode::EXCLUSIVE,
            &[family],
        )?;

        Ok(())
    }

    fn create_picture
    (
        &self,
        format: &vk::VideoFormatPropertiesKHR,
        usage: vk::ImageUsageFlags,
        extent: vk::Extent2D,
        layers: u32,
        sharing: vk::SharingMode,
        families: &[u32],
    ) -> Result<Picture, String>
    {
        let mut list = self.profile.list();

        let info = vk::ImageCreateInfo::default()
            .flags(format.image_create_flags)
            .image_type(format.image_type)
            .format(format.format)
            .extent(vk::Extent3D { width: extent.width, height: extent.height, depth: 1 })
            .mip_levels(1)
            .array_layers(layers)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(format.image_tiling)
            .usage(usage)
            .sharing_mode(sharing)
            .queue_family_indices(families)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut list);

        let mut picture = Picture::default();

        //SAFETY: FREED ON FAILURE
        let made = unsafe
        {
            (|| -> Result<(), vk::Result>
            {
                picture.image = self.device.create_image(&info, None)?;

                let needs = self.device.get_image_memory_requirements(picture.image);
                picture.memory = self.allocate(needs, vk::MemoryPropertyFlags::DEVICE_LOCAL, vk::MemoryPropertyFlags::empty())
                    .map_err(|_| vk::Result::ERROR_OUT_OF_DEVICE_MEMORY)?;

                self.device.bind_image_memory(picture.image, picture.memory, 0)?;

                let view_type = if layers > 1 { vk::ImageViewType::TYPE_2D_ARRAY } else { vk::ImageViewType::TYPE_2D };

                let range = vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .level_count(1)
                    .layer_count(layers);

                picture.view = self.device.create_image_view(&vk::ImageViewCreateInfo::default()
                    .image(picture.image)
                    .view_type(view_type)
                    .format(format.format)
                    .subresource_range(range), None)?;

                Ok(())
            })()
        };

        if let Err(error) = made
        {
            self.destroy_picture(&picture);
            return Err(error.to_string());
        }

        Ok(picture)
    }

    fn create_buffers(&mut self, candidate: &Candidate) -> Result<(), String>
    {
        let pixels = u64::from(self.coded.width) * u64::from(self.coded.height);

        self.staging = self.create_mapped
        (
            pixels * 3 / 2,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            vk::MemoryPropertyFlags::empty(),
            false,
        )?;

        let size = (pixels * 3 / 2 + BITSTREAM_SLACK).next_multiple_of(candidate.size_alignment.max(1));

        self.bitstream = self.create_mapped
        (
            size,
            vk::BufferUsageFlags::VIDEO_ENCODE_DST_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE,
            vk::MemoryPropertyFlags::HOST_CACHED,
            true,
        )?;

        Ok(())
    }

    fn create_mapped
    (
        &self,
        size: u64,
        usage: vk::BufferUsageFlags,
        required: vk::MemoryPropertyFlags,
        preferred: vk::MemoryPropertyFlags,
        video: bool,
    ) -> Result<Mapped, String>
    {
        let mut list = self.profile.list();

        let mut info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        if video { info = info.push_next(&mut list); }

        let mut mapped = Mapped { size, ..Mapped::default() };

        //SAFETY: FREED ON FAILURE
        let made = unsafe
        {
            (|| -> Result<(), vk::Result>
            {
                mapped.buffer = self.device.create_buffer(&info, None)?;

                let needs = self.device.get_buffer_memory_requirements(mapped.buffer);
                let index = memory_type(&self.memory_properties, needs.memory_type_bits, required, preferred)
                    .ok_or(vk::Result::ERROR_OUT_OF_DEVICE_MEMORY)?;

                mapped.coherent = self.memory_properties.memory_types[index as usize].property_flags.contains(vk::MemoryPropertyFlags::HOST_COHERENT);
                mapped.memory = self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(needs.size).memory_type_index(index), None)?;

                self.device.bind_buffer_memory(mapped.buffer, mapped.memory, 0)?;

                mapped.pointer = self.device.map_memory(mapped.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())? as *mut u8;

                Ok(())
            })()
        };

        if let Err(error) = made
        {
            self.destroy_mapped(&mapped);
            return Err(error.to_string());
        }

        Ok(mapped)
    }

    fn create_feedback(&mut self) -> Result<(), String>
    {
        let mut feedback = vk::QueryPoolVideoEncodeFeedbackCreateInfoKHR::default()
            .encode_feedback_flags(vk::VideoEncodeFeedbackFlagsKHR::BITSTREAM_BUFFER_OFFSET | vk::VideoEncodeFeedbackFlagsKHR::BITSTREAM_BYTES_WRITTEN);

        //PROFILE BEHIND THE FEEDBACK FLAGS
        feedback.p_next = &self.profile.info as *const _ as *const c_void;

        let mut info = vk::QueryPoolCreateInfo::default()
            .query_type(vk::QueryType::VIDEO_ENCODE_FEEDBACK_KHR)
            .query_count(1);

        info.p_next = &feedback as *const _ as *const c_void;

        //SAFETY: info OUTLIVES THE CALL
        self.feedback = unsafe { self.device.create_query_pool(&info, None) }.map_err(|error| error.to_string())?;

        Ok(())
    }

    fn video_format(&self, physical: vk::PhysicalDevice, usage: vk::ImageUsageFlags) -> Result<vk::VideoFormatPropertiesKHR<'static>, String>
    {
        let video = khr::video_queue::Instance::new(&self.entry, &self.instance);

        let mut list = self.profile.list();
        let info = vk::PhysicalDeviceVideoFormatInfoKHR::default().image_usage(usage).push_next(&mut list);

        let fp = video.fp();
        let mut count = 0;

        //SAFETY: formats HOLDS count
        let formats = unsafe
        {
            (fp.get_physical_device_video_format_properties_khr)(physical, &info, &mut count, ptr::null_mut())
                .result()
                .map_err(|error| error.to_string())?;

            let mut formats = vec![vk::VideoFormatPropertiesKHR::default(); count as usize];

            (fp.get_physical_device_video_format_properties_khr)(physical, &info, &mut count, formats.as_mut_ptr())
                .result()
                .map_err(|error| error.to_string())?;

            formats.truncate(count as usize);
            formats
        };

        formats.into_iter()
            .find(|format| format.format == vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .ok_or_else(|| "the encoder takes no NV12".to_owned())
    }

    fn allocate(&self, needs: vk::MemoryRequirements, required: vk::MemoryPropertyFlags, preferred: vk::MemoryPropertyFlags) -> Result<vk::DeviceMemory, String>
    {
        let index = memory_type(&self.memory_properties, needs.memory_type_bits, required, preferred)
            .ok_or_else(|| "no memory type fits".to_owned())?;

        //SAFETY: CALLER FREES IT
        unsafe { self.device.allocate_memory(&vk::MemoryAllocateInfo::default().allocation_size(needs.size).memory_type_index(index), None) }
            .map_err(|error| error.to_string())
    }

    fn destroy_picture(&self, picture: &Picture)
    {
        //SAFETY: NULLS SKIPPED
        unsafe
        {
            if picture.view != vk::ImageView::null() { self.device.destroy_image_view(picture.view, None); }
            if picture.image != vk::Image::null() { self.device.destroy_image(picture.image, None); }
            if picture.memory != vk::DeviceMemory::null() { self.device.free_memory(picture.memory, None); }
        }
    }

    fn destroy_mapped(&self, mapped: &Mapped)
    {
        //SAFETY: NULLS SKIPPED
        unsafe
        {
            if mapped.buffer != vk::Buffer::null() { self.device.destroy_buffer(mapped.buffer, None); }
            if mapped.memory != vk::DeviceMemory::null() { self.device.free_memory(mapped.memory, None); }
        }
    }

    fn upload(&mut self, planes: &Planes) -> Result<(), String>
    {
        let coded = (self.coded.width as usize, self.coded.height as usize);
        let luma_size = coded.0 * coded.1;

        //SAFETY: MAPPED STAGING BUFFER
        let staging = unsafe { slice::from_raw_parts_mut(self.staging.pointer, self.staging.size as usize) };
        let (luma, chroma) = staging.split_at_mut(luma_size);

        encoder::write_nv12(planes, luma, coded.0, chroma, coded.0, coded);

        let layer = |aspect| vk::ImageSubresourceLayers::default().aspect_mask(aspect).layer_count(1);

        let regions =
        [
            vk::BufferImageCopy::default()
                .buffer_row_length(self.coded.width)
                .buffer_image_height(self.coded.height)
                .image_subresource(layer(vk::ImageAspectFlags::PLANE_0))
                .image_extent(vk::Extent3D { width: self.coded.width, height: self.coded.height, depth: 1 }),

            vk::BufferImageCopy::default()
                .buffer_offset(luma_size as u64)
                .buffer_row_length(self.coded.width / 2)
                .buffer_image_height(self.coded.height / 2)
                .image_subresource(layer(vk::ImageAspectFlags::PLANE_1))
                .image_extent(vk::Extent3D { width: self.coded.width / 2, height: self.coded.height / 2, depth: 1 }),
        ];

        let barrier = vk::ImageMemoryBarrier2::default()
            .dst_stage_mask(vk::PipelineStageFlags2::COPY)
            .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.source.image)
            .subresource_range(color_range(1));

        let device = &self.device;
        let commands = self.upload_commands;

        //SAFETY: LAST ENCODE FINISHED
        unsafe
        {
            device.begin_command_buffer(commands, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))
                .map_err(|error| error.to_string())?;

            device.cmd_pipeline_barrier2(commands, &vk::DependencyInfo::default().image_memory_barriers(slice::from_ref(&barrier)));
            device.cmd_copy_buffer_to_image(commands, self.staging.buffer, self.source.image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &regions);

            device.end_command_buffer(commands).map_err(|error| error.to_string())?;

            let submit = vk::SubmitInfo::default()
                .command_buffers(slice::from_ref(&commands))
                .signal_semaphores(slice::from_ref(&self.uploaded));

            device.queue_submit(self.upload_queue, slice::from_ref(&submit), vk::Fence::null()).map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    fn h264_rate(&self, controlled: bool) -> vk::VideoEncodeH264RateControlInfoKHR<'static>
    {
        vk::VideoEncodeH264RateControlInfoKHR::default()
            .flags(vk::VideoEncodeH264RateControlFlagsKHR::REFERENCE_PATTERN_FLAT)
            .gop_frame_count(self.period)
            .idr_period(self.period)
            .temporal_layer_count(u32::from(controlled))
    }

    fn rate<'a>(&self, layers: &'a [vk::VideoEncodeRateControlLayerInfoKHR<'a>]) -> vk::VideoEncodeRateControlInfoKHR<'a>
    {
        let controlled = !layers.is_empty();

        vk::VideoEncodeRateControlInfoKHR::default()
            .rate_control_mode(self.rate_mode)
            .layers(layers)
            .virtual_buffer_size_in_ms(if controlled { 1000 } else { 0 })
            .initial_virtual_buffer_size_in_ms(if controlled { 500 } else { 0 })
    }

    fn collect(&self) -> Result<Vec<u8>, String> //LAST ENCODE'S BYTES
    {
        let (offset, written) = if self.status
        {
            let mut results = [[0u32; 3]; 1];

            //SAFETY: FENCE SIGNALLED
            unsafe { self.device.get_query_pool_results(self.feedback, 0, &mut results, vk::QueryResultFlags::WITH_STATUS_KHR) }
                .map_err(|error| error.to_string())?;

            if results[0][2] as i32 != STATUS_COMPLETE { return Err(format!("the encode failed ({})", results[0][2] as i32)); }

            (results[0][0], results[0][1])
        } else
        {
            let mut results = [[0u32; 2]; 1];

            //SAFETY: FENCE SIGNALLED
            unsafe { self.device.get_query_pool_results(self.feedback, 0, &mut results, vk::QueryResultFlags::empty()) }
                .map_err(|error| error.to_string())?;

            (results[0][0], results[0][1])
        };

        let (offset, written) = (offset as usize, written as usize);

        if offset + written > self.bitstream.size as usize { return Err("the encoder overran its buffer".to_owned()); }

        if !self.bitstream.coherent
        {
            let range = vk::MappedMemoryRange::default().memory(self.bitstream.memory).size(vk::WHOLE_SIZE);

            //SAFETY: WHOLE MAPPING
            unsafe { self.device.invalidate_mapped_memory_ranges(slice::from_ref(&range)) }.map_err(|error| error.to_string())?;
        }

        //SAFETY: BOUNDS CHECKED ABOVE
        let bytes = unsafe { slice::from_raw_parts(self.bitstream.pointer.add(offset), written) };

        Ok(bytes.to_vec())
    }
}

impl Backend for VulkanEncoder
{
    fn encode(&mut self, planes: &Planes, keyframe: bool) -> Result<Vec<u8>, String>
    {
        //OVER BUDGET - SKIP
        if !self.budget.allows(keyframe) { return Ok(Vec::new()); }

        let previous = match self.reference
        {
            Some(reference) if !keyframe && self.since_idr + 1 < self.period => Some(reference),
            _ => None,
        };

        let idr = previous.is_none();
        let since_idr = if idr { 0 } else { self.since_idr + 1 };
        let frame_num = since_idr % MAX_FRAME_NUM;
        let poc = since_idr as i32 * 2;
        let slot = previous.map_or(0, |reference| 1 - reference.slot);

        self.upload(planes)?;

        //SAFETY: PLAIN C STRUCTS
        let (mut setup_std, mut reference_std, mut lists, mut picture, mut header) = unsafe
        {
            (
                mem::zeroed::<native::StdVideoEncodeH264ReferenceInfo>(),
                mem::zeroed::<native::StdVideoEncodeH264ReferenceInfo>(),
                mem::zeroed::<native::StdVideoEncodeH264ReferenceListsInfo>(),
                mem::zeroed::<native::StdVideoEncodeH264PictureInfo>(),
                mem::zeroed::<native::StdVideoEncodeH264SliceHeader>(),
            )
        };

        let picture_type = if idr
        {
            native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_IDR
        } else
        {
            native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_P
        };

        setup_std.primary_pic_type = picture_type;
        setup_std.FrameNum = frame_num;
        setup_std.PicOrderCnt = poc;

        lists.RefPicList0 = [NO_REFERENCE; 32];
        lists.RefPicList1 = [NO_REFERENCE; 32];

        if let Some(reference) = previous
        {
            reference_std.primary_pic_type = if reference.idr
            {
                native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_IDR
            } else
            {
                native::StdVideoH264PictureType_STD_VIDEO_H264_PICTURE_TYPE_P
            };

            reference_std.FrameNum = reference.frame_num;
            reference_std.PicOrderCnt = reference.poc;

            lists.RefPicList0[0] = reference.slot as u8;
        }

        picture.flags.set_IdrPicFlag(u32::from(idr));
        picture.flags.set_is_reference(1);
        picture.idr_pic_id = self.idr_pic_id;
        picture.primary_pic_type = picture_type;
        picture.frame_num = frame_num;
        picture.PicOrderCnt = poc;
        picture.pRefLists = &lists;

        header.slice_type = if idr
        {
            native::StdVideoH264SliceType_STD_VIDEO_H264_SLICE_TYPE_I
        } else
        {
            native::StdVideoH264SliceType_STD_VIDEO_H264_SLICE_TYPE_P
        };

        let slice_info = vk::VideoEncodeH264NaluSliceInfoKHR::default().std_slice_header(&header);

        let mut h264_picture = vk::VideoEncodeH264PictureInfoKHR::default()
            .nalu_slice_entries(slice::from_ref(&slice_info))
            .std_picture_info(&picture);

        let mut setup_dpb = vk::VideoEncodeH264DpbSlotInfoKHR::default().std_reference_info(&setup_std);
        let mut reference_dpb = vk::VideoEncodeH264DpbSlotInfoKHR::default().std_reference_info(&reference_std);

        let setup_resource = vk::VideoPictureResourceInfoKHR::default()
            .coded_extent(self.coded)
            .base_array_layer(slot)
            .image_view_binding(self.dpb.view);

        let reference_resource = vk::VideoPictureResourceInfoKHR::default()
            .coded_extent(self.coded)
            .base_array_layer(previous.map_or(0, |reference| reference.slot))
            .image_view_binding(self.dpb.view);

        let source_resource = vk::VideoPictureResourceInfoKHR::default()
            .coded_extent(self.coded)
            .image_view_binding(self.source.view);

        let setup_slot = vk::VideoReferenceSlotInfoKHR::default()
            .slot_index(slot as i32)
            .picture_resource(&setup_resource)
            .push_next(&mut setup_dpb);

        let reference_slot = vk::VideoReferenceSlotInfoKHR::default()
            .slot_index(previous.map_or(-1, |reference| reference.slot as i32))
            .picture_resource(&reference_resource)
            .push_next(&mut reference_dpb);

        //SETUP PICTURE, NOT YET ACTIVE
        let unbound = vk::VideoReferenceSlotInfoKHR::default()
            .slot_index(-1)
            .picture_resource(&setup_resource);

        let bound = if previous.is_some() { vec![reference_slot, unbound] } else { vec![unbound] };
        let references = if previous.is_some() { slice::from_ref(&reference_slot) } else { &[] };

        let encode_info = vk::VideoEncodeInfoKHR::default()
            .dst_buffer(self.bitstream.buffer)
            .dst_buffer_range(self.bitstream.size)
            .src_picture_resource(source_resource)
            .setup_reference_slot(&setup_slot)
            .reference_slots(references)
            .push_next(&mut h264_picture);

        //RATE CONTROL, AS HELD AND AS WANTED
        let controlled = self.rate_mode != vk::VideoEncodeRateControlModeFlagsKHR::DEFAULT;

        let (mut h264_layer, mut h264_layer_next) = (vk::VideoEncodeH264RateControlLayerInfoKHR::default(), vk::VideoEncodeH264RateControlLayerInfoKHR::default());

        let layer = rate_layer(self.applied, self.fps).push_next(&mut h264_layer);
        let layer_next = rate_layer(self.bitrate, self.fps).push_next(&mut h264_layer_next);

        let (mut h264_rate, mut h264_rate_next) = (self.h264_rate(controlled), self.h264_rate(controlled));

        let mut rate = self.rate(if controlled { slice::from_ref(&layer) } else { &[] });
        let mut rate_next = self.rate(if controlled { slice::from_ref(&layer_next) } else { &[] });

        let retarget = controlled && self.reset && self.applied != self.bitrate;

        let source_barrier = vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
            .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
            .dst_access_mask(vk::AccessFlags2::VIDEO_ENCODE_READ_KHR)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::VIDEO_ENCODE_SRC_KHR)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.source.image)
            .subresource_range(color_range(1));

        let dpb_barrier = vk::ImageMemoryBarrier2::default()
            .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
            .dst_access_mask(vk::AccessFlags2::VIDEO_ENCODE_READ_KHR | vk::AccessFlags2::VIDEO_ENCODE_WRITE_KHR)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::VIDEO_ENCODE_DPB_KHR)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.dpb.image)
            .subresource_range(color_range(DPB_SLOTS));

        //LAST RECONSTRUCTION, NOW A REFERENCE
        let reconstructed = vk::MemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
            .src_access_mask(vk::AccessFlags2::VIDEO_ENCODE_WRITE_KHR)
            .dst_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
            .dst_access_mask(vk::AccessFlags2::VIDEO_ENCODE_READ_KHR | vk::AccessFlags2::VIDEO_ENCODE_WRITE_KHR);

        //BITSTREAM TO THE HOST
        let written = vk::MemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::VIDEO_ENCODE_KHR)
            .src_access_mask(vk::AccessFlags2::VIDEO_ENCODE_WRITE_KHR)
            .dst_stage_mask(vk::PipelineStageFlags2::HOST)
            .dst_access_mask(vk::AccessFlags2::HOST_READ);

        let images = if self.dpb_ready { vec![source_barrier] } else { vec![source_barrier, dpb_barrier] };

        let device = &self.device;
        let commands = self.encode_commands;
        let video = self.video.fp();

        //SAFETY: FENCE WAITED BELOW
        unsafe
        {
            device.begin_command_buffer(commands, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT))
                .map_err(|error| error.to_string())?;

            device.cmd_reset_query_pool(commands, self.feedback, 0, 1);

            device.cmd_pipeline_barrier2(commands, &vk::DependencyInfo::default()
                .memory_barriers(slice::from_ref(&reconstructed))
                .image_memory_barriers(&images));

            let mut begin = vk::VideoBeginCodingInfoKHR::default()
                .video_session(self.session)
                .video_session_parameters(self.parameters)
                .reference_slots(&bound);

            if self.reset && controlled
            {
                begin = begin.push_next(&mut rate).push_next(&mut h264_rate);
            }

            (video.cmd_begin_video_coding_khr)(commands, &begin);

            //FIRST ENCODE RESETS THE SESSION
            if !self.reset
            {
                let mut flags = vk::VideoCodingControlFlagsKHR::RESET;

                if controlled { flags |= vk::VideoCodingControlFlagsKHR::ENCODE_RATE_CONTROL; }

                let mut control = vk::VideoCodingControlInfoKHR::default().flags(flags);

                if controlled { control = control.push_next(&mut rate_next).push_next(&mut h264_rate_next); }

                (video.cmd_control_video_coding_khr)(commands, &control);
            } else if retarget
            {
                //A NEW TARGET
                let control = vk::VideoCodingControlInfoKHR::default()
                    .flags(vk::VideoCodingControlFlagsKHR::ENCODE_RATE_CONTROL)
                    .push_next(&mut rate_next)
                    .push_next(&mut h264_rate_next);

                (video.cmd_control_video_coding_khr)(commands, &control);
            }

            device.cmd_begin_query(commands, self.feedback, 0, vk::QueryControlFlags::empty());
            (self.video_encode.fp().cmd_encode_video_khr)(commands, &encode_info);
            device.cmd_end_query(commands, self.feedback, 0);

            (video.cmd_end_video_coding_khr)(commands, &vk::VideoEndCodingInfoKHR::default());

            device.cmd_pipeline_barrier2(commands, &vk::DependencyInfo::default().memory_barriers(slice::from_ref(&written)));

            device.end_command_buffer(commands).map_err(|error| error.to_string())?;

            let stage = vk::PipelineStageFlags::ALL_COMMANDS;

            let submit = vk::SubmitInfo::default()
                .wait_semaphores(slice::from_ref(&self.uploaded))
                .wait_dst_stage_mask(slice::from_ref(&stage))
                .command_buffers(slice::from_ref(&commands));

            device.queue_submit(self.encode_queue, slice::from_ref(&submit), self.encoded).map_err(|error| error.to_string())?;

            let waited = device.wait_for_fences(slice::from_ref(&self.encoded), true, FENCE_TIMEOUT);

            waited.map_err(|error| error.to_string())?;

            device.reset_fences(slice::from_ref(&self.encoded)).map_err(|error| error.to_string())?;
        }

        self.reset = true;
        self.dpb_ready = true;
        self.applied = self.bitrate;

        let bitstream = self.collect()?;

        self.budget.spend(bitstream.len());

        self.reference = Some(Reference { slot, frame_num, poc, idr });
        self.since_idr = since_idr;

        if !idr { return Ok(bitstream); }

        self.idr_pic_id = self.idr_pic_id.wrapping_add(1);

        let mut frame = Vec::with_capacity(self.headers.len() + bitstream.len());
        frame.extend_from_slice(&self.headers);
        frame.extend_from_slice(&bitstream);

        Ok(frame)
    }

    fn set_bitrate(&mut self, bitrate: u32)
    {
        self.bitrate = u64::from(bitrate).min(self.max_bitrate);
        self.budget.set_rate(bitrate);
    }

    fn hardware(&self) -> bool
    {
        true
    }
}

impl Drop for VulkanEncoder
{
    fn drop(&mut self)
    {
        //SAFETY: DEVICE IDLE, NULLS SKIPPED
        unsafe
        {
            self.device.device_wait_idle().ok();

            let handle = self.device.handle();
            let video = self.video.fp();

            if self.feedback != vk::QueryPool::null() { self.device.destroy_query_pool(self.feedback, None); }

            self.destroy_mapped(&self.bitstream);
            self.destroy_mapped(&self.staging);
            self.destroy_picture(&self.dpb);
            self.destroy_picture(&self.source);

            if self.parameters != vk::VideoSessionParametersKHR::null() { (video.destroy_video_session_parameters_khr)(handle, self.parameters, ptr::null()); }
            if self.session != vk::VideoSessionKHR::null() { (video.destroy_video_session_khr)(handle, self.session, ptr::null()); }

            for memory in self.session_memory.drain(..) { self.device.free_memory(memory, None); }

            if self.encoded != vk::Fence::null() { self.device.destroy_fence(self.encoded, None); }
            if self.uploaded != vk::Semaphore::null() { self.device.destroy_semaphore(self.uploaded, None); }
            if self.encode_pool != vk::CommandPool::null() { self.device.destroy_command_pool(self.encode_pool, None); }
            if self.upload_pool != vk::CommandPool::null() { self.device.destroy_command_pool(self.upload_pool, None); }

            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

//FUNCTIONS
fn select(entry: &Entry, instance: &Instance, profile: &Profile) -> Result<Candidate, String> //FIRST GPU THAT CAN ENCODE
{
    let video = khr::video_queue::Instance::new(entry, instance);

    //SAFETY: A LIVE INSTANCE
    let mut physicals = unsafe { instance.enumerate_physical_devices() }.map_err(|error| error.to_string())?;

    //DISCRETE CARDS FIRST
    //SAFETY: OUR HANDLES
    physicals.sort_by_key(|physical| unsafe { instance.get_physical_device_properties(*physical) }.device_type != vk::PhysicalDeviceType::DISCRETE_GPU);

    let mut last = "no GPU does Vulkan video encoding".to_owned();

    for physical in physicals
    {
        match candidate(instance, &video, physical, profile)
        {
            Ok(candidate) => return Ok(candidate),
            Err(error) => last = error,
        }
    }

    Err(last)
}

fn candidate(instance: &Instance, video: &khr::video_queue::Instance, physical: vk::PhysicalDevice, profile: &Profile) -> Result<Candidate, String>
{
    //SAFETY: OUR PHYSICAL DEVICE
    let properties = unsafe { instance.get_physical_device_properties(physical) };

    if properties.api_version < vk::API_VERSION_1_3 { return Err("the GPU predates Vulkan 1.3".to_owned()); }

    //SAFETY: OUR PHYSICAL DEVICE
    let extensions = unsafe { instance.enumerate_device_extension_properties(physical) }.map_err(|error| error.to_string())?;

    for wanted in [khr::video_queue::NAME, khr::video_encode_queue::NAME, khr::video_encode_h264::NAME]
    {
        if !extensions.iter().any(|extension| extension.extension_name_as_c_str() == Ok(wanted))
        {
            return Err(format!("the GPU lacks {}", wanted.to_string_lossy()));
        }
    }

    //SAFETY: OUR PHYSICAL DEVICE
    let count = unsafe { instance.get_physical_device_queue_family_properties2_len(physical) };

    let mut videos = vec![vk::QueueFamilyVideoPropertiesKHR::default(); count];
    let mut statuses = vec![vk::QueueFamilyQueryResultStatusPropertiesKHR::default(); count];

    let flags: Vec<vk::QueueFlags> =
    {
        let mut families: Vec<vk::QueueFamilyProperties2> = videos.iter_mut().zip(statuses.iter_mut())
            .map(|(video, status)| vk::QueueFamilyProperties2::default().push_next(video).push_next(status))
            .collect();

        //SAFETY: families HOLDS count
        unsafe { instance.get_physical_device_queue_family_properties2(physical, &mut families) };

        families.iter().map(|family| family.queue_family_properties.queue_flags).collect()
    };

    let encode_family = (0..count)
        .find(|&index| flags[index].contains(vk::QueueFlags::VIDEO_ENCODE_KHR)
            && videos[index].video_codec_operations.contains(vk::VideoCodecOperationFlagsKHR::ENCODE_H264))
        .ok_or_else(|| "no queue encodes H.264".to_owned())?;

    //ENCODE QUEUE IF IT CAN COPY
    let upload_family = if flags[encode_family].intersects(vk::QueueFlags::TRANSFER | vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
    {
        encode_family
    } else
    {
        (0..count)
            .find(|&index| flags[index].intersects(vk::QueueFlags::TRANSFER | vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE))
            .ok_or_else(|| "no queue can upload".to_owned())?
    };

    let mut h264_caps = vk::VideoEncodeH264CapabilitiesKHR::default();
    let mut encode_caps = vk::VideoEncodeCapabilitiesKHR::default();
    let mut caps = vk::VideoCapabilitiesKHR::default().push_next(&mut encode_caps).push_next(&mut h264_caps);

    //SAFETY: LOCALS OUTLIVE THE CALL
    unsafe { (video.fp().get_physical_device_video_capabilities_khr)(physical, &profile.info, &mut caps) }
        .result()
        .map_err(|error| format!("constrained baseline is not offered ({error})"))?;

    let (size_alignment, granularity, min_extent, max_extent, max_dpb, max_refs) =
    (
        caps.min_bitstream_buffer_size_alignment,
        caps.picture_access_granularity,
        caps.min_coded_extent,
        caps.max_coded_extent,
        caps.max_dpb_slots,
        caps.max_active_reference_pictures,
    );

    if max_dpb < DPB_SLOTS || max_refs < 1 || h264_caps.max_p_picture_l0_reference_count < 1
    {
        return Err("the encoder cannot hold a reference".to_owned());
    }

    let granularity = vk::Extent2D
    {
        width: granularity.width.max(encode_caps.encode_input_picture_granularity.width),
        height: granularity.height.max(encode_caps.encode_input_picture_granularity.height),
    };

    Ok(Candidate
    {
        physical,
        encode_family: encode_family as u32,
        upload_family: upload_family as u32,
        status: statuses[encode_family].query_result_status_support == vk::TRUE,
        size_alignment,
        granularity,
        min_extent,
        max_extent,
        rate_modes: encode_caps.rate_control_modes,
        max_bitrate: encode_caps.max_bitrate,
        feedback: encode_caps.supported_encode_feedback_flags,
        max_level: h264_caps.max_level_idc,
    })
}

fn create_device(instance: &Instance, candidate: &Candidate) -> Result<Device, String>
{
    let priorities = [1.0f32];

    let mut queues = vec![vk::DeviceQueueCreateInfo::default().queue_family_index(candidate.encode_family).queue_priorities(&priorities)];

    if candidate.upload_family != candidate.encode_family
    {
        queues.push(vk::DeviceQueueCreateInfo::default().queue_family_index(candidate.upload_family).queue_priorities(&priorities));
    }

    let extensions = [khr::video_queue::NAME.as_ptr(), khr::video_encode_queue::NAME.as_ptr(), khr::video_encode_h264::NAME.as_ptr()];

    let mut features = vk::PhysicalDeviceVulkan13Features::default().synchronization2(true);

    let info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queues)
        .enabled_extension_names(&extensions)
        .push_next(&mut features);

    //SAFETY: info OUTLIVES THE CALL
    unsafe { instance.create_device(candidate.physical, &info, None) }.map_err(|error| error.to_string())
}

fn level(coded: vk::Extent2D, fps: u32, max: native::StdVideoH264LevelIdc) -> Result<native::StdVideoH264LevelIdc, String> //LOWEST FITTING LEVEL
{
    let macroblocks = u64::from(coded.width / 16) * u64::from(coded.height / 16);

    LEVELS.iter()
        .find(|(_, frame, second)| macroblocks <= *frame && macroblocks * u64::from(fps) <= *second)
        .map(|(level, _, _)| *level)
        .filter(|level| *level <= max || max == native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_1_0) //1.0 MEANS UNREPORTED
        .ok_or_else(|| format!("{}x{} needs a level the encoder lacks", coded.width, coded.height))
}

fn rate_layer<'a>(bitrate: u64, fps: u32) -> vk::VideoEncodeRateControlLayerInfoKHR<'a> //ONE RATE CONTROL LAYER
{
    vk::VideoEncodeRateControlLayerInfoKHR::default()
        .average_bitrate(bitrate)
        .max_bitrate(bitrate)
        .frame_rate_numerator(fps)
        .frame_rate_denominator(1)
}

fn color_range(layers: u32) -> vk::ImageSubresourceRange
{
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(layers)
}

fn memory_type
(
    properties: &vk::PhysicalDeviceMemoryProperties,
    bits: u32,
    required: vk::MemoryPropertyFlags,
    preferred: vk::MemoryPropertyFlags,
) -> Option<u32> //required, preferred IF POSSIBLE
{
    let types = &properties.memory_types[..properties.memory_type_count as usize];
    let fits = |index: usize, flags: vk::MemoryPropertyFlags| bits & (1 << index) != 0 && types[index].property_flags.contains(flags);

    (0..types.len()).find(|&index| fits(index, required | preferred))
        .or_else(|| (0..types.len()).find(|&index| fits(index, required)))
        .map(|index| index as u32)
}
