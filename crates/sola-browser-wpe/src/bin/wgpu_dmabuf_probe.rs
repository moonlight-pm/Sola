//! Phase-0a probe — answer the question: **can we import an
//! externally-allocated Vulkan image (DMA-BUF backed) into wgpu as
//! a sampleable `wgpu::Texture` inside iced's `shader::Program`
//! pipeline?**
//!
//! No WPE involved. The probe creates a `VkImage` in iced's own
//! `VkDevice` with `VkExportMemoryAllocateInfo`, gets back an FD via
//! `vkGetMemoryFdKHR`, writes a known checkerboard into its memory
//! via direct mapping, then allocates a SECOND `VkImage` that imports
//! the FD via `VkImportMemoryFdInfoKHR`. The imported image is wrapped
//! as a `wgpu::Texture` via `wgpu_hal::vulkan::Device::texture_from_raw`
//! → `wgpu::Device::create_texture_from_hal`, then sampled in a
//! fullscreen-triangle render pass.
//!
//! Success criterion: an 8×8 magenta/cyan checkerboard fills the
//! window with no artifacts and stable colors across frames. If it
//! renders, the import path is real and every later phase can rely on
//! it.

use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};

use ash::vk;
use iced::widget::{Shader, shader};
use iced::{Element, Length, Rectangle, Task};

const IMG_W: u32 = 256;
const IMG_H: u32 = 256;
/// `VK_FORMAT_B8G8R8A8_UNORM` — matches wgpu `Bgra8Unorm`. UNORM (not
/// SRGB) so the byte values we write are the colors that render.
const VK_FMT: vk::Format = vk::Format::B8G8R8A8_UNORM;
const WGPU_FMT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
/// 8 cells per row at IMG_W=256 → 32px squares.
const CELL: u32 = 32;

fn main() -> iced::Result {
    sola_core::log::init("wgpu-dmabuf-probe");
    tracing::info!("wgpu-dmabuf-probe starting");

    let _ = sola_core::env::activate_wayland_session(10_000);

    iced::application(App::default, App::update, App::view)
        .title(|_: &App| "wgpu-dmabuf-probe".into())
        .window(iced::window::Settings {
            decorations: false,
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "wgpu-dmabuf-probe".into(),
                ..Default::default()
            },
            ..iced::window::Settings::default()
        })
        .run()
}

#[derive(Default)]
struct App;

#[derive(Debug, Clone)]
enum Msg {}

impl App {
    fn update(&mut self, _: Msg) -> Task<Msg> {
        Task::none()
    }

    fn view(&self) -> Element<'_, Msg> {
        Shader::new(CheckerboardProgram)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

// ---- iced shader plumbing ----------------------------------------

#[derive(Debug)]
struct CheckerboardProgram;

impl shader::Program<Msg> for CheckerboardProgram {
    type State = ();
    type Primitive = CheckerboardPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: iced::mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        CheckerboardPrimitive
    }
}

#[derive(Debug)]
struct CheckerboardPrimitive;

impl shader::Primitive for CheckerboardPrimitive {
    type Pipeline = CheckerboardPipeline;

    fn prepare(
        &self,
        _pipeline: &mut Self::Pipeline,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &iced::widget::shader::Viewport,
    ) {
        // Texture + pipeline are built once in `Pipeline::new`. No
        // per-frame work — the imported texture's content doesn't
        // change in this probe.
    }

    fn render(
        &self,
        pipeline: &Self::Pipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dmabuf-probe sample pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_scissor_rect(
            clip_bounds.x,
            clip_bounds.y,
            clip_bounds.width,
            clip_bounds.height,
        );
        pass.set_pipeline(&pipeline.render_pipeline);
        pass.set_bind_group(0, &pipeline.bind_group, &[]);
        // Fullscreen triangle via gl_VertexIndex tricks in the vertex
        // shader — no vertex/index buffers needed.
        pass.draw(0..3, 0..1);
    }
}

// ---- pipeline construction ---------------------------------------

#[derive(Debug)]
struct CheckerboardPipeline {
    render_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    /// Holds the imported wgpu::Texture and the Vulkan handles it
    /// depends on. Field order matters for drop: `wgpu_texture` runs
    /// first (releases the imported VkImage via wgpu-hal), then
    /// `producer` (frees both memories + the export image + the FD).
    _imported: ImportedDmabuf,
}

impl shader::Pipeline for CheckerboardPipeline {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let imported = build_imported_dmabuf(device);
        let view = imported
            .wgpu_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("dmabuf-probe sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("dmabuf-probe bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dmabuf-probe bg"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dmabuf-probe shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dmabuf-probe pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dmabuf-probe rp"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            render_pipeline,
            bind_group,
            _imported: imported,
        }
    }
}

const SHADER_WGSL: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Fullscreen triangle via vertex index trick — three vertices cover
// the viewport with no vertex/index buffers.
@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var out: VsOut;
    let x = f32((vid << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vid & 2u) * 2.0 - 1.0;
    out.pos = vec4<f32>(x, -y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}
"#;

// ---- Vulkan: allocate, export, write, import, wrap ---------------

/// Holds the wgpu texture wrapping the imported VkImage, plus the
/// raw Vulkan handles whose lifetimes the wgpu texture depends on.
struct ImportedDmabuf {
    wgpu_texture: wgpu::Texture,
    /// Producer-side handles + the import memory. wgpu-hal destroys
    /// the import VkImage itself when `wgpu_texture` drops (we passed
    /// `drop_callback = None` into `texture_from_raw`). Everything
    /// here is what's left for us to clean up.
    _producer: ProducerHandles,
}

impl std::fmt::Debug for ImportedDmabuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportedDmabuf")
            .field("wgpu_texture", &self.wgpu_texture)
            .field("producer", &self._producer)
            .finish()
    }
}

struct ProducerHandles {
    device: ash::Device,
    export_image: vk::Image,
    export_memory: vk::DeviceMemory,
    import_memory: vk::DeviceMemory,
    _fd: OwnedFd,
}

impl std::fmt::Debug for ProducerHandles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProducerHandles")
            .field("export_image", &self.export_image)
            .field("export_memory", &self.export_memory)
            .field("import_memory", &self.import_memory)
            .finish()
    }
}

impl Drop for ProducerHandles {
    fn drop(&mut self) {
        // SAFETY: device is still valid — wgpu owns it via the wgpu
        // texture that this struct outlives. Handles were created
        // by us and have not been freed yet.
        unsafe {
            self.device.destroy_image(self.export_image, None);
            self.device.free_memory(self.export_memory, None);
            self.device.free_memory(self.import_memory, None);
        }
    }
}

fn build_imported_dmabuf(device: &wgpu::Device) -> ImportedDmabuf {
    // All Vulkan work happens inside the `as_hal` closure so we never
    // outlive the guard. Only the resulting `wgpu_hal::vulkan::Texture`
    // (owned) and the `ProducerHandles` (which only borrows the ash
    // device handle, which is cheap to clone and reference-counted)
    // escape.
    let (hal_texture, producer) = unsafe {
        let guard = device
            .as_hal::<wgpu_hal::api::Vulkan>()
            .expect("wgpu device is not a Vulkan device — probe requires Vulkan backend");

        let ash_device: ash::Device = guard.raw_device().clone();
        let physical = guard.raw_physical_device();
        let instance: &ash::Instance = guard.shared_instance().raw_instance();

        // 1. Allocate the producer-side VkImage with external memory.
        let (export_image, export_memory) =
            allocate_export_image(instance, physical, &ash_device);

        // 2. Write the checkerboard via direct memory mapping.
        write_checkerboard(&ash_device, export_memory);

        // 3. Export an FD for the memory.
        let fd = export_memory_fd(instance, &ash_device, export_memory);

        // 4. Allocate the consumer-side VkImage importing that FD.
        let import_fd = fd.try_clone().expect("dup fd");
        let (import_image, import_memory) =
            allocate_import_image(instance, physical, &ash_device, import_fd);

        // 5. Hand the import VkImage to wgpu-hal. With `None` drop
        // callback, wgpu-hal destroys this VkImage when the resulting
        // Texture drops — saves us from tracking it.
        let hal_texture = guard.texture_from_raw(
            import_image,
            &wgpu_hal::TextureDescriptor {
                label: Some("dmabuf-probe imported"),
                size: wgpu::Extent3d {
                    width: IMG_W,
                    height: IMG_H,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: WGPU_FMT,
                usage: wgpu::TextureUses::RESOURCE,
                memory_flags: wgpu_hal::MemoryFlags::empty(),
                view_formats: vec![],
            },
            None,
        );

        let producer = ProducerHandles {
            device: ash_device,
            export_image,
            export_memory,
            import_memory,
            _fd: fd,
        };

        (hal_texture, producer)
    };

    let wgpu_texture = unsafe {
        device.create_texture_from_hal::<wgpu_hal::api::Vulkan>(
            hal_texture,
            &wgpu::TextureDescriptor {
                label: Some("dmabuf-probe imported wgpu"),
                size: wgpu::Extent3d {
                    width: IMG_W,
                    height: IMG_H,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: WGPU_FMT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        )
    };

    ImportedDmabuf {
        wgpu_texture,
        _producer: producer,
    }
}

// ---- ash helpers --------------------------------------------------

unsafe fn allocate_export_image(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    device: &ash::Device,
) -> (vk::Image, vk::DeviceMemory) {
    let mut external_info = vk::ExternalMemoryImageCreateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

    let image_info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(VK_FMT)
        .extent(vk::Extent3D { width: IMG_W, height: IMG_H, depth: 1 })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::LINEAR)
        .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::PREINITIALIZED)
        .push_next(&mut external_info);

    let image = unsafe { device.create_image(&image_info, None) }
        .expect("vkCreateImage (export)");

    let mem_reqs = unsafe { device.get_image_memory_requirements(image) };
    let mem_type_idx = find_memory_type(
        instance,
        physical,
        mem_reqs.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    );

    let mut export_alloc = vk::ExportMemoryAllocateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

    let alloc_info = vk::MemoryAllocateInfo::default()
        .allocation_size(mem_reqs.size)
        .memory_type_index(mem_type_idx)
        .push_next(&mut export_alloc);

    let memory = unsafe { device.allocate_memory(&alloc_info, None) }
        .expect("vkAllocateMemory (export)");

    unsafe { device.bind_image_memory(image, memory, 0) }
        .expect("vkBindImageMemory (export)");

    tracing::info!(
        size = mem_reqs.size,
        type_idx = mem_type_idx,
        "allocated export VkImage"
    );

    (image, memory)
}

unsafe fn write_checkerboard(device: &ash::Device, memory: vk::DeviceMemory) {
    let ptr = unsafe {
        device
            .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
            .expect("vkMapMemory")
    } as *mut u8;
    // BGRA byte order in memory matches Vk format B8G8R8A8_UNORM.
    for y in 0..IMG_H {
        for x in 0..IMG_W {
            let cell_x = x / CELL;
            let cell_y = y / CELL;
            let (b, g, r, a) = if (cell_x + cell_y) & 1 == 0 {
                (0xFF, 0x00, 0xFF, 0xFF) // magenta
            } else {
                (0xFF, 0xFF, 0x00, 0xFF) // cyan
            };
            let off = ((y * IMG_W + x) * 4) as usize;
            unsafe {
                ptr.add(off).write(b);
                ptr.add(off + 1).write(g);
                ptr.add(off + 2).write(r);
                ptr.add(off + 3).write(a);
            }
        }
    }
    unsafe { device.unmap_memory(memory) };
    tracing::info!("wrote checkerboard to export memory");
}

unsafe fn export_memory_fd(
    instance: &ash::Instance,
    device: &ash::Device,
    memory: vk::DeviceMemory,
) -> OwnedFd {
    let ext = ash::khr::external_memory_fd::Device::new(instance, device);
    let info = vk::MemoryGetFdInfoKHR::default()
        .memory(memory)
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let raw = unsafe { ext.get_memory_fd(&info) }.expect("vkGetMemoryFdKHR");
    tracing::info!(fd = raw, "exported memory fd");
    unsafe { OwnedFd::from_raw_fd(raw) }
}

unsafe fn allocate_import_image(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    device: &ash::Device,
    fd: OwnedFd,
) -> (vk::Image, vk::DeviceMemory) {
    let mut external_info = vk::ExternalMemoryImageCreateInfo::default()
        .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

    let image_info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(VK_FMT)
        .extent(vk::Extent3D { width: IMG_W, height: IMG_H, depth: 1 })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::LINEAR)
        .usage(vk::ImageUsageFlags::SAMPLED)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::PREINITIALIZED)
        .push_next(&mut external_info);

    let image = unsafe { device.create_image(&image_info, None) }
        .expect("vkCreateImage (import)");

    let mem_reqs = unsafe { device.get_image_memory_requirements(image) };
    let mem_type_idx = find_memory_type(
        instance,
        physical,
        mem_reqs.memory_type_bits,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    );

    // vkAllocateMemory takes ownership of the FD on success; into_raw_fd
    // releases it from OwnedFd's drop guard so we don't double-close.
    let raw_fd = fd.into_raw_fd();
    let mut import_info = vk::ImportMemoryFdInfoKHR::default()
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
        .fd(raw_fd);

    let alloc_info = vk::MemoryAllocateInfo::default()
        .allocation_size(mem_reqs.size)
        .memory_type_index(mem_type_idx)
        .push_next(&mut import_info);

    let memory = unsafe { device.allocate_memory(&alloc_info, None) }
        .expect("vkAllocateMemory (import)");

    unsafe { device.bind_image_memory(image, memory, 0) }
        .expect("vkBindImageMemory (import)");

    tracing::info!(fd = raw_fd, "imported memory fd into VkImage");

    (image, memory)
}

fn find_memory_type(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    type_bits: u32,
    properties: vk::MemoryPropertyFlags,
) -> u32 {
    let props = unsafe { instance.get_physical_device_memory_properties(physical) };
    for i in 0..props.memory_type_count {
        if (type_bits & (1 << i)) != 0
            && props.memory_types[i as usize]
                .property_flags
                .contains(properties)
        {
            return i;
        }
    }
    panic!(
        "no suitable memory type for type_bits={type_bits:#x} properties={properties:?}"
    );
}
