//! DMA-BUF → `wgpu::Texture` import for frames coming out of WPE.
//!
//! Built on the path validated by `wgpu-dmabuf-probe` (phase 0a):
//! `wgpu_hal::vulkan::Device::texture_from_raw` wraps a VkImage we
//! created, then `wgpu::Device::create_texture_from_hal` produces a
//! public `wgpu::Texture`. Difference vs the probe: now the FD comes
//! from another process, and the producer's modifier (NVIDIA's
//! `0x300000000606014` in the WPE case) is non-LINEAR — but the
//! reported stride matches what LINEAR would have, and ad-hoc
//! mmap+read produced correct pixels in `wpe-probe`, so we try
//! LINEAR import first and only escalate to
//! `VK_EXT_image_drm_format_modifier` if that breaks.
//!
//! The returned `ImportedFrame` owns the VkImage (destroyed by
//! wgpu-hal when `texture` drops) and the VkDeviceMemory (we free
//! it in the `_holder` Drop). The FD is consumed by
//! `vkAllocateMemory(VkImportMemoryFdInfoKHR)` and closed by the
//! driver when memory is freed — no separate `close` needed.

use std::os::fd::{IntoRawFd, OwnedFd};

use ash::vk;

/// Public handle returned by `import`. Drop order matters: `texture`
/// runs first (releases the imported VkImage via wgpu-hal), then
/// `_holder` (frees the imported VkDeviceMemory).
pub struct ImportedFrame {
    pub texture: wgpu::Texture,
    _holder: MemoryHolder,
}

impl std::fmt::Debug for ImportedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportedFrame")
            .field("texture", &self.texture)
            .finish_non_exhaustive()
    }
}

struct MemoryHolder {
    device: ash::Device,
    memory: vk::DeviceMemory,
}

impl Drop for MemoryHolder {
    fn drop(&mut self) {
        // SAFETY: device clones share refcount via ash internals;
        // memory was allocated by us and not yet freed.
        unsafe { self.device.free_memory(self.memory, None) };
    }
}

pub struct DmabufMetadata {
    pub width: u32,
    pub height: u32,
    /// DRM fourcc. Currently only ARGB8888 (`0x34325241`) is wired.
    pub format: u32,
    pub modifier: u64,
    pub stride: u32,
    pub offset: u32,
}

/// Import `fd` as a sampleable `wgpu::Texture`. Takes ownership of
/// the FD — `vkAllocateMemory` consumes it and the driver closes it
/// when the memory is freed.
///
/// SAFETY: `device` must be a Vulkan-backed wgpu device.
pub unsafe fn import(
    device: &wgpu::Device,
    fd: OwnedFd,
    meta: &DmabufMetadata,
) -> Result<ImportedFrame, ImportError> {
    // DRM fourcc 'AR24' = ARGB8888 (with alpha), 'XR24' = XRGB8888
    // (alpha bits ignored). Both lay out as BGRA / BGRX bytes in
    // memory — wgpu's Bgra8Unorm samples them identically; treating
    // XRGB as BGRA just means the implicit-1.0 alpha channel comes
    // from the X bits, which is what we want.
    if meta.format != 0x3432_5241 && meta.format != 0x3432_5258 {
        return Err(ImportError::UnsupportedFormat(meta.format));
    }
    let vk_format = vk::Format::B8G8R8A8_UNORM;
    let wgpu_format = wgpu::TextureFormat::Bgra8Unorm;

    let raw_fd = fd.into_raw_fd();

    let (texture, memory) = unsafe {
        let guard = device
            .as_hal::<wgpu_hal::api::Vulkan>()
            .ok_or(ImportError::NotVulkanBackend)?;
        let ash_device: ash::Device = guard.raw_device().clone();
        let physical = guard.raw_physical_device();
        let instance: &ash::Instance = guard.shared_instance().raw_instance();

        // Keep the warning around so future-us notices if WPE ever
        // sends a non-LINEAR buffer past `WPE_BUFFER_FORMAT=AR24:0:scanout`
        // (set in main.rs). wgpu doesn't enable
        // VK_EXT_image_drm_format_modifier from the public API; we
        // sample as LINEAR, which silently produces tile-pattern
        // garbage if the source isn't actually linear.
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| {
            if meta.modifier != 0 {
                tracing::warn!(
                    modifier = format!("{:#x}", meta.modifier),
                    "imported DMA-BUF has non-LINEAR modifier — \
                     wgpu samples it as LINEAR and will render garbage. \
                     Check that WPE_BUFFER_FORMAT=AR24:0:scanout is in effect."
                );
            }
        });

        // Create the consumer-side VkImage referencing external
        // memory. LINEAR tiling for now — see module doc for the
        // rationale (WPE-on-NVIDIA's reported modifier is non-zero
        // but the buffer is effectively linear at our pitch). The
        // texture_from_raw + create_texture_from_hal call gets us
        // a wgpu::Texture wrapping it.
        let mut external_info = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk_format)
            .extent(vk::Extent3D {
                width: meta.width,
                height: meta.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::LINEAR)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::PREINITIALIZED)
            .push_next(&mut external_info);

        let image = ash_device
            .create_image(&image_info, None)
            .map_err(ImportError::Vulkan)?;

        let mem_reqs = ash_device.get_image_memory_requirements(image);
        // Pick a HOST_VISIBLE memory type — same as the probe. The
        // imported FD lives in whatever memory the producer chose;
        // we just need a memory type the driver accepts for import.
        let mem_type_idx = find_memory_type(
            instance,
            physical,
            mem_reqs.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )
        .ok_or(ImportError::NoSuitableMemoryType)?;

        let mut import_info = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .fd(raw_fd);

        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_reqs.size)
            .memory_type_index(mem_type_idx)
            .push_next(&mut import_info);

        let memory = ash_device
            .allocate_memory(&alloc_info, None)
            .map_err(ImportError::Vulkan)?;

        ash_device
            .bind_image_memory(image, memory, 0)
            .map_err(ImportError::Vulkan)?;

        // Pre-transition the import VkImage to SHADER_READ_ONLY_OPTIMAL
        // before wgpu sees it. wgpu-core hardcodes imported textures'
        // tracker state to UNINITIALIZED → its first-use barrier
        // transitions from `oldLayout = UNDEFINED`, which per Vulkan
        // spec allows the driver to discard contents. NVIDIA's
        // proprietary driver appears permissive for CPU-written
        // memory (0a's checkerboard worked without this) but
        // genuinely discards for GPU-written DMA-BUFs coming from
        // another context (WPE's GPU process here, in 0c → black
        // window). By transitioning explicitly to the target layout
        // first, wgpu's redundant UNDEFINED → READ barrier becomes
        // a no-op on the GPU side and contents survive.
        transition_to_shader_read(
            &ash_device,
            guard.raw_queue(),
            guard.queue_family_index(),
            image,
        );

        let hal_texture = guard.texture_from_raw(
            image,
            &wgpu_hal::TextureDescriptor {
                label: Some("wpe-imported"),
                size: wgpu::Extent3d {
                    width: meta.width,
                    height: meta.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_format,
                usage: wgpu::TextureUses::RESOURCE,
                memory_flags: wgpu_hal::MemoryFlags::empty(),
                view_formats: vec![],
            },
            None,
        );

        let texture = device.create_texture_from_hal::<wgpu_hal::api::Vulkan>(
            hal_texture,
            &wgpu::TextureDescriptor {
                label: Some("wpe-imported wgpu"),
                size: wgpu::Extent3d {
                    width: meta.width,
                    height: meta.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );

        (
            texture,
            MemoryHolder {
                device: ash_device,
                memory,
            },
        )
    };

    Ok(ImportedFrame {
        texture,
        _holder: memory,
    })
}

#[derive(Debug)]
pub enum ImportError {
    NotVulkanBackend,
    UnsupportedFormat(u32),
    NoSuitableMemoryType,
    Vulkan(vk::Result),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::NotVulkanBackend => write!(f, "wgpu is not on the Vulkan backend"),
            ImportError::UnsupportedFormat(fmt) => {
                write!(f, "unsupported DRM format {:#x}", fmt)
            }
            ImportError::NoSuitableMemoryType => write!(f, "no host-visible memory type"),
            ImportError::Vulkan(r) => write!(f, "Vulkan error: {r:?}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// One-shot transition of the imported VkImage from PREINITIALIZED
/// (the layout we created it in) to SHADER_READ_ONLY_OPTIMAL.
/// Synchronous: submits with a fence and waits before returning so
/// the queue is in a known state by the time wgpu samples the
/// texture. Same pattern as the phase-0a probe's fix that turned
/// out to be unnecessary there but is needed here (GPU-written
/// source vs CPU-written source).
unsafe fn transition_to_shader_read(
    device: &ash::Device,
    queue: vk::Queue,
    queue_family: u32,
    image: vk::Image,
) {
    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::TRANSIENT)
        .queue_family_index(queue_family);
    let pool = device
        .create_command_pool(&pool_info, None)
        .expect("vkCreateCommandPool (transition)");

    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let cmds = device
        .allocate_command_buffers(&alloc_info)
        .expect("vkAllocateCommandBuffers (transition)");
    let cmd = cmds[0];

    device
        .begin_command_buffer(
            cmd,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )
        .expect("vkBeginCommandBuffer (transition)");

    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::PREINITIALIZED)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });

    device.cmd_pipeline_barrier(
        cmd,
        vk::PipelineStageFlags::TOP_OF_PIPE,
        vk::PipelineStageFlags::FRAGMENT_SHADER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[barrier],
    );
    device
        .end_command_buffer(cmd)
        .expect("vkEndCommandBuffer (transition)");

    let fence = device
        .create_fence(&vk::FenceCreateInfo::default(), None)
        .expect("vkCreateFence (transition)");
    let submit = vk::SubmitInfo::default().command_buffers(&cmds);
    device
        .queue_submit(queue, &[submit], fence)
        .expect("vkQueueSubmit (transition)");
    device
        .wait_for_fences(&[fence], true, u64::MAX)
        .expect("vkWaitForFences (transition)");
    device.destroy_fence(fence, None);
    device.destroy_command_pool(pool, None);
}

fn find_memory_type(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    type_bits: u32,
    properties: vk::MemoryPropertyFlags,
) -> Option<u32> {
    let props = unsafe { instance.get_physical_device_memory_properties(physical) };
    (0..props.memory_type_count).find(|&i| {
        (type_bits & (1 << i)) != 0
            && props.memory_types[i as usize]
                .property_flags
                .contains(properties)
    })
}
