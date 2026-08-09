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
    // sRGB-encoded color. WPE's WebProcess renders sRGB pixels and
    // iced's swapchain target is sRGB-encoded too, so we need the
    // GPU to sRGB-decode on sample (Bgra8UnormSrgb) — sampling as
    // linear (Bgra8Unorm) would chain two sRGB encodes and produce
    // a washed-out / desaturated frame.
    let vk_format = vk::Format::B8G8R8A8_SRGB;
    let wgpu_format = wgpu::TextureFormat::Bgra8UnormSrgb;

    let raw_fd = fd.into_raw_fd();

    let (texture, memory) = unsafe {
        let guard = device
            .as_hal::<wgpu_hal::api::Vulkan>()
            .ok_or(ImportError::NotVulkanBackend)?;
        let ash_device: ash::Device = guard.raw_device().clone();
        let physical = guard.raw_physical_device();
        let instance: &ash::Instance = guard.shared_instance().raw_instance();

        // Verify our wgpu-hal fork enabled VK_EXT_image_drm_format_modifier.
        // If not, modifier-aware vkCreateImage will fail with
        // VK_ERROR_FORMAT_NOT_SUPPORTED.
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| {
            let exts = guard.enabled_device_extensions();
            let has_modifier = exts
                .iter()
                .any(|c| c.to_bytes() == b"VK_EXT_image_drm_format_modifier");
            tracing::info!(
                has_modifier,
                "wgpu Vulkan device has VK_EXT_image_drm_format_modifier",
            );
            if !has_modifier {
                tracing::warn!(
                    "VK_EXT_image_drm_format_modifier NOT enabled — \
                     the wgpu-hal patch at crates/wgpu-hal-patched isn't \
                     in effect (check [patch.crates-io] in Cargo.toml)."
                );
            }
        });

        // Create the consumer-side VkImage referencing external
        // memory. For non-LINEAR modifiers we have to use
        // DRM_FORMAT_MODIFIER_EXT tiling + push the explicit
        // modifier-info struct onto the pNext chain so the driver
        // knows the tile layout. For LINEAR (modifier == 0) we
        // could in theory use either path; we use the modifier
        // path uniformly so we have a single code path.
        let mut external_info = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

        let plane_layouts = [vk::SubresourceLayout {
            offset: meta.offset as u64,
            size: 0, /* must be 0 per VUID-VkSubresourceLayout-size-09604 */
            row_pitch: meta.stride as u64,
            array_pitch: 0,
            depth_pitch: 0,
        }];
        let mut modifier_info = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
            .drm_format_modifier(meta.modifier)
            .plane_layouts(&plane_layouts);

        // Per VK_EXT_image_drm_format_modifier spec the initial
        // layout MUST be UNDEFINED when tiling is
        // DRM_FORMAT_MODIFIER_EXT. wgpu's first-use barrier will
        // transition UNDEFINED → SHADER_READ_ONLY_OPTIMAL, which
        // for modifier-tagged images preserves the imported
        // content (unlike the LINEAR + PREINITIALIZED case which
        // needed our explicit pre-transition to survive the
        // "discard contents" semantics of UNDEFINED on NVIDIA).
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
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external_info)
            .push_next(&mut modifier_info);

        let image = ash_device
            .create_image(&image_info, None)
            .map_err(ImportError::Vulkan)?;

        let mem_reqs = ash_device.get_image_memory_requirements(image);
        // Memory type: a DEVICE_LOCAL type is correct for
        // GPU-rendered DMA-BUFs. Falls back to "anything that
        // matches type_bits" if the driver doesn't expose
        // DEVICE_LOCAL for this allocation type.
        let mem_type_idx = find_memory_type(
            instance,
            physical,
            mem_reqs.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )
        .or_else(|| {
            find_memory_type(
                instance,
                physical,
                mem_reqs.memory_type_bits,
                vk::MemoryPropertyFlags::empty(),
            )
        })
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
