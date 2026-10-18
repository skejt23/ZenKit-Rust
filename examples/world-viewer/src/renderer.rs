use crate::{
    Options,
    assets::{Assets, key},
    camera::Camera,
    scene::Scene,
};
use anyhow::{Context, Result};
use std::{collections::HashMap, path::Path};
use wgpu::util::DeviceExt;

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const BACKGROUND: wgpu::Color = wgpu::Color {
    r: 0.16,
    g: 0.22,
    b: 0.29,
    a: 1.0,
};

struct Batch {
    vertices: wgpu::Buffer,
    instances: wgpu::Buffer,
    vertex_count: u32,
    instance_count: u32,
    material: usize,
}
pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    opaque: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    materials: Vec<wgpu::BindGroup>,
    blended: Vec<bool>,
    batches: Vec<Batch>,
    depth: wgpu::TextureView,
}

pub fn instance() -> wgpu::Instance {
    wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default())
}

impl Renderer {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        scene: &Scene,
        assets: &Assets,
        width: u32,
        height: u32,
    ) -> Result<(Self, wgpu::TextureFormat, wgpu::Adapter)> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await
            .context("no compatible GPU adapter")?;
        let info = adapter.get_info();
        eprintln!("GPU: {} ({:?})", info.name, info.backend);
        let format = match surface {
            Some(s) => s
                .get_capabilities(&adapter)
                .formats
                .into_iter()
                .find(|f| f.is_srgb())
                .context("surface has no sRGB format")?,
            None => wgpu::TextureFormat::Rgba8UnormSrgb,
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Gothic viewer"),
                ..Default::default()
            })
            .await
            .context("creating GPU device")?;
        let shader = device.create_shader_module(wgpu::include_wgsl!("world.wgsl"));
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("repeat trilinear"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mut materials = vec![];
        let mut texture_cache = HashMap::<Vec<u8>, wgpu::TextureView>::new();
        let mut missing = 0;
        for material in &scene.materials {
            let name = key(&material.texture);
            let view = texture_cache.entry(name).or_insert_with(|| {
                let pixels = match assets.texture(&material.texture) {
                    Ok(Some(image)) => image,
                    Ok(None) => {
                        if !material.texture.is_empty() {
                            missing += 1;
                        }
                        image::RgbaImage::from_pixel(1, 1, image::Rgba([255; 4]))
                    }
                    Err(e) => {
                        missing += 1;
                        eprintln!(
                            "Texture {}: {e:#}",
                            String::from_utf8_lossy(&material.texture)
                        );
                        image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 80, 255, 255]))
                    }
                };
                upload_texture(&device, &queue, pixels)
            });
            materials.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material"),
                layout: &texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            }));
        }
        eprintln!(
            "Uploaded {} textures ({missing} missing/failed; fallback colors used)",
            texture_cache.len()
        );
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world pipeline"),
            bind_group_layouts: &[&camera_layout, &texture_layout],
            push_constant_ranges: &[],
        });
        let pipeline = |blend: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if blend {
                    "transparent world"
                } else {
                    "opaque world"
                }),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[
                        wgpu::VertexBufferLayout {
                            array_stride: 48,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![
                                0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4
                            ],
                        },
                        wgpu::VertexBufferLayout {
                            array_stride: 64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &wgpu::vertex_attr_array![
                                4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4
                            ],
                        },
                    ],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                // Draw both sides of foliage and thin sheets.
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: !blend,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview: None,
                cache: None,
            })
        };
        let opaque = pipeline(false);
        let blend = pipeline(true);
        let mut batches = vec![];
        for primitive in &scene.primitives {
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh"),
                contents: bytemuck::cast_slice(&primitive.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("VOB transforms"),
                contents: bytemuck::cast_slice(&primitive.instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
            batches.push(Batch {
                vertices,
                instances,
                vertex_count: primitive.vertices.len().try_into()?,
                instance_count: primitive.instances.len().try_into()?,
                material: primitive.material,
            });
        }
        let depth = depth_texture(&device, width, height);
        Ok((
            Self {
                device,
                queue,
                opaque,
                blend,
                camera_buffer,
                camera_group,
                materials,
                blended: scene.materials.iter().map(|m| m.blend).collect(),
                batches,
                depth,
            },
            format,
            adapter,
        ))
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        self.depth = depth_texture(&self.device, width, height);
    }
    pub fn draw(
        &self,
        target: &wgpu::TextureView,
        camera: &Camera,
        width: u32,
        height: u32,
    ) -> wgpu::CommandEncoder {
        let mut uniform = camera
            .matrix(width as f32 / height as f32)
            .to_cols_array()
            .to_vec();
        uniform.extend([camera.position.x, camera.position.y, camera.position.z, 0.0]);
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&uniform));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("world frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(BACKGROUND),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.camera_group, &[]);
            for transparent in [false, true] {
                pass.set_pipeline(if transparent {
                    &self.blend
                } else {
                    &self.opaque
                });
                for batch in &self.batches {
                    if self.blended[batch.material] != transparent {
                        continue;
                    }
                    pass.set_bind_group(1, &self.materials[batch.material], &[]);
                    pass.set_vertex_buffer(0, batch.vertices.slice(..));
                    pass.set_vertex_buffer(1, batch.instances.slice(..));
                    pass.draw(0..batch.vertex_count, 0..batch.instance_count);
                }
            }
        }
        encoder
    }
}

fn depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut image: image::RgbaImage,
) -> wgpu::TextureView {
    let (width, height) = image.dimensions();
    let levels = width.max(height).ilog2() + 1;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("diffuse"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for level in 0..levels {
        let (width, height) = image.dimensions();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            image.as_raw(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        if level + 1 < levels {
            image = image::imageops::resize(
                &image,
                (width / 2).max(1),
                (height / 2).max(1),
                image::imageops::FilterType::Triangle,
            );
        }
    }
    texture.create_view(&Default::default())
}

pub async fn screenshot(
    scene: &Scene,
    assets: &Assets,
    camera: &Camera,
    options: &Options,
    path: &Path,
) -> Result<()> {
    let instance = instance();
    let (renderer, format, _) = Renderer::new(
        &instance,
        None,
        scene,
        assets,
        options.width,
        options.height,
    )
    .await?;
    let size = wgpu::Extent3d {
        width: options.width,
        height: options.height,
        depth_or_array_layers: 1,
    };
    let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("capture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = (options.width * 4).div_ceil(256) * 256;
    let readback = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture readback"),
        size: u64::from(row) * u64::from(options.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = renderer.draw(
        &target.create_view(&Default::default()),
        camera,
        options.width,
        options.height,
    );
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(options.height),
            },
        },
        size,
    );
    renderer.queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = sender.send(r);
    });
    renderer.device.poll(wgpu::PollType::wait_indefinitely())?;
    receiver.recv()??;
    let mapped = readback.slice(..).get_mapped_range();
    let pixels: Vec<u8> = mapped
        .chunks_exact(row as usize)
        .flat_map(|r| &r[..options.width as usize * 4])
        .copied()
        .collect();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    image::save_buffer_with_format(
        path,
        &pixels,
        options.width,
        options.height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    drop(mapped);
    readback.unmap();
    eprintln!("Saved {}", path.display());
    Ok(())
}
