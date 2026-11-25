//! The thin layer over wgpu that the engine draws with: textures, one shared pass layout,
//! and reading results back.

use std::collections::HashMap;

pub const PARAMS: usize = 24; // vec4 entries in a pass's block of numbers
const PARAM_BYTES: u64 = (PARAMS * 16) as u64;
const SLOT_BYTES: u64 = 512;
const SLOTS: u64 = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pass {
    Expose,
    Blur,
    Down,
    Combine,
    Develop,
    Grain,
    Paper,
    Tone,
    Mip,
    Canvas,
}

impl Pass {
    const ALL: [Pass; 10] = [Pass::Expose, Pass::Blur, Pass::Down, Pass::Combine, Pass::Develop, Pass::Grain, Pass::Paper, Pass::Tone, Pass::Mip, Pass::Canvas];
    fn entry(self) -> &'static str {
        match self {
            Pass::Expose => "fs_expose",
            Pass::Blur => "fs_blur",
            Pass::Down => "fs_down",
            Pass::Combine => "fs_combine",
            Pass::Develop => "fs_develop",
            Pass::Grain => "fs_grain",
            Pass::Paper => "fs_paper",
            Pass::Tone => "fs_tone",
            Pass::Mip => "fs_mip",
            Pass::Canvas => "fs_canvas",
        }
    }
}

/// How many bits the working pictures hold per value. Half is plenty for the eye and
/// halves memory traffic; Full is there to check the arithmetic against the Python tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    Half,
    Full,
}

pub struct Tex {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub w: u32,
    pub h: u32,
    pub format: wgpu::TextureFormat,
    pub mips: u32,
}

impl Tex {
    pub fn bytes(&self) -> u64 {
        let px = match self.format {
            wgpu::TextureFormat::R16Float => 2,
            wgpu::TextureFormat::Rg16Float | wgpu::TextureFormat::R32Float | wgpu::TextureFormat::Rgba8UnormSrgb => 4,
            wgpu::TextureFormat::Rg32Float => 8,
            _ => 16,
        };
        let base = self.w as u64 * self.h as u64 * px;
        if self.mips > 1 {
            base * 4 / 3
        } else {
            base
        }
    }

    /// Texture coordinate per pixel of a picture `factor` times this one's size.
    pub fn per_pixel(&self, factor: f32) -> [f32; 2] {
        [1.0 / (factor * self.w as f32), 1.0 / (factor * self.h as f32)]
    }
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub max_dim: u32,
    pub f1: wgpu::TextureFormat, // one value per pixel
    pub f2: wgpu::TextureFormat, // two values per pixel
    layout: wgpu::BindGroupLayout,
    pipelines: HashMap<(Pass, wgpu::TextureFormat), wgpu::RenderPipeline>,
    pipeline_layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    slot: u64,
    blank: Tex,
    blank_table: Tex,
    readback: Option<(u64, wgpu::Buffer)>,
}

/// Features worth asking a device for when the adapter has them.
pub fn wanted_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features() & wgpu::Features::FLOAT32_FILTERABLE
}

impl Gpu {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, precision: Precision) -> Gpu {
        let full = precision == Precision::Full && device.features().contains(wgpu::Features::FLOAT32_FILTERABLE);
        let (f1, f2) = if full {
            (wgpu::TextureFormat::R32Float, wgpu::TextureFormat::Rg32Float)
        } else {
            (wgpu::TextureFormat::R16Float, wgpu::TextureFormat::Rg16Float)
        };
        let tex_entry = |binding: u32, filterable: bool| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let mut entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(PARAM_BYTES) },
                count: None,
            },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ];
        for b in 2..8 {
            entries.push(tex_entry(b, true));
        }
        for b in 8..10 {
            entries.push(tex_entry(b, false));
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("pass"), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pass"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("stages"), source: wgpu::ShaderSource::Wgsl(include_str!("shaders/stages.wgsl").into()) });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mirror"),
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::MirrorRepeat,
            address_mode_w: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor { label: Some("params"), size: SLOT_BYTES * SLOTS, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let max_dim = device.limits().max_texture_dimension_2d;
        let mut gpu = Gpu {
            blank: make_tex(&device, 1, 1, f1, 1),
            blank_table: make_tex(&device, 1, 1, wgpu::TextureFormat::R32Float, 1),
            device,
            queue,
            max_dim,
            f1,
            f2,
            layout,
            pipelines: HashMap::new(),
            pipeline_layout,
            module,
            sampler,
            params,
            slot: 0,
            readback: None,
        };
        // build every pipeline up front so the first drag of a slider does not wait on a compile
        for pass in Pass::ALL {
            let formats: &[wgpu::TextureFormat] = match pass {
                Pass::Blur | Pass::Down | Pass::Combine => &[f1, f2],
                Pass::Grain => &[f2],
                Pass::Tone => &[f1, wgpu::TextureFormat::R32Float],
                Pass::Canvas => &[wgpu::TextureFormat::Rgba8UnormSrgb, wgpu::TextureFormat::Rgba32Float],
                _ => &[f1],
            };
            for &f in formats {
                gpu.pipeline(pass, f);
            }
        }
        gpu
    }

    fn pipeline(&mut self, pass: Pass, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        let (device, module, layout) = (&self.device, &self.module, &self.pipeline_layout);
        self.pipelines.entry((pass, format)).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(pass.entry()),
                layout: Some(layout),
                vertex: wgpu::VertexState { module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(pass.entry()),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    pub fn tex(&self, w: u32, h: u32, format: wgpu::TextureFormat) -> Tex {
        make_tex(&self.device, w, h, format, 1)
    }

    /// A picture with the full chain of half-size copies under it.
    pub fn tex_with_mips(&self, w: u32, h: u32, format: wgpu::TextureFormat) -> Tex {
        let mips = 32 - w.max(h).leading_zeros();
        make_tex(&self.device, w, h, format, mips)
    }

    pub fn table(&self, data: &[f32]) -> Tex {
        let t = make_tex(&self.device, data.len() as u32, 1, wgpu::TextureFormat::R32Float, 1);
        self.write_table(&t, data);
        t
    }

    pub fn write_table(&self, t: &Tex, data: &[f32]) {
        self.write(t, bytemuck::cast_slice(data), 4);
    }

    pub fn write(&self, t: &Tex, bytes: &[u8], bytes_per_px: u32) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &t.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(t.w * bytes_per_px), rows_per_image: Some(t.h) },
            wgpu::Extent3d { width: t.w, height: t.h, depth_or_array_layers: 1 },
        );
    }

    pub fn encoder(&self) -> wgpu::CommandEncoder {
        self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None })
    }

    /// Hand the recorded passes to the GPU. Numbers and tables written since the last
    /// submit belong to these passes, so each picture is submitted before the next begins.
    pub fn submit(&mut self, enc: wgpu::CommandEncoder) {
        self.queue.submit([enc.finish()]);
        self.slot = 0;
    }

    /// Record one full-screen pass into `target`.
    #[allow(clippy::too_many_arguments)]
    pub fn pass(&mut self, enc: &mut wgpu::CommandEncoder, pass: Pass, target: &wgpu::TextureView, format: wgpu::TextureFormat, numbers: &[[f32; 4]], pictures: &[&wgpu::TextureView], tables: &[&wgpu::TextureView]) {
        assert!(numbers.len() <= PARAMS && pictures.len() <= 6 && tables.len() <= 2);
        assert!(self.slot < SLOTS, "too many passes in one submit");
        let mut block = [[0f32; 4]; PARAMS];
        block[..numbers.len()].copy_from_slice(numbers);
        let offset = self.slot * SLOT_BYTES;
        self.slot += 1;
        self.queue.write_buffer(&self.params, offset, bytemuck::cast_slice(&block));
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.params, offset: 0, size: wgpu::BufferSize::new(PARAM_BYTES) }) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
        ];
        for i in 0..6 {
            entries.push(wgpu::BindGroupEntry { binding: 2 + i as u32, resource: wgpu::BindingResource::TextureView(pictures.get(i).copied().unwrap_or(&self.blank.view)) });
        }
        for i in 0..2 {
            entries.push(wgpu::BindGroupEntry { binding: 8 + i as u32, resource: wgpu::BindingResource::TextureView(tables.get(i).copied().unwrap_or(&self.blank_table.view)) });
        }
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &self.layout, entries: &entries });
        self.pipeline(pass, format);
        let pipeline = &self.pipelines[&(pass, format)];
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(pass.entry()),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, &group, &[offset as u32]);
        rp.draw(0..3, 0..1);
    }

    /// Wait for the GPU and return the texels of `t` (level 0), row by row.
    pub fn read(&mut self, t: &Tex, bytes_per_px: u32) -> Vec<u8> {
        let row = t.w * bytes_per_px;
        let padded = row.div_ceil(256) * 256;
        let size = padded as u64 * t.h as u64;
        if self.readback.as_ref().is_none_or(|b| b.0 < size) {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            self.readback = Some((size, buffer));
        }
        let buffer = &self.readback.as_ref().unwrap().1;
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &t.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(t.h) } },
            wgpu::Extent3d { width: t.w, height: t.h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let slice = buffer.slice(..size);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok();
        let mut out = Vec::with_capacity((row * t.h) as usize);
        {
            let data = slice.get_mapped_range().unwrap();
            for y in 0..t.h as usize {
                let start = y * padded as usize;
                out.extend_from_slice(&data[start..start + row as usize]);
            }
        }
        buffer.unmap();
        out
    }

    /// Block until everything submitted so far has been drawn.
    pub fn finish(&self) {
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok();
    }
}

fn make_tex(device: &wgpu::Device, w: u32, h: u32, format: wgpu::TextureFormat, mips: u32) -> Tex {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Tex { texture, view, w: w.max(1), h: h.max(1), format, mips }
}

/// A device without a window, for tests and tools. None when the machine has no usable GPU.
pub fn headless() -> Option<(wgpu::Device, wgpu::Queue, String)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, force_fallback_adapter: false, ..Default::default() }))
        .or_else(|_| pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::LowPower, force_fallback_adapter: true, ..Default::default() })))
        .ok()?;
    let info = adapter.get_info();
    let limits = wgpu::Limits { max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d.min(16384), ..wgpu::Limits::default() };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("studio"),
        required_features: wanted_features(&adapter),
        required_limits: limits,
        memory_hints: wgpu::MemoryHints::Performance,
        ..Default::default()
    }))
    .ok()?;
    Some((device, queue, format!("{} ({:?})", info.name, info.backend)))
}
