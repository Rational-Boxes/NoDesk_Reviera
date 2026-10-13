// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Reviera contributors

//! The built-in `wgpu` viewport backend (TECH_STACK §3.2, spec Ch 5 DOC-RBE-001).
//!
//! Draws a [`RenderScene`] with flat shading and a headlight: opaque geometry first, then
//! transparent geometry (glass, spaces) blended on top. It renders into a native window surface or
//! into an offscreen texture (tests, thumbnails).

use bytemuck::{Pod, Zeroable};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use reviera_render::{Camera, RenderBackend, RenderScene};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
}

const SHADER: &str = r#"
struct U { view_proj: mat4x4<f32>, eye: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = u.view_proj * vec4<f32>(p, 1.0);
    o.normal = n;
    o.color = c;
    o.world = p;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let n = normalize(i.normal);
    let v = normalize(u.eye.xyz - i.world);
    let key = normalize(vec3<f32>(0.4, 0.3, 0.85));
    // Two-sided: faces lit whichever way they point.
    let light = 0.25 + 0.40 * abs(dot(n, v)) + 0.35 * abs(dot(n, key));
    return vec4<f32>(i.color.rgb * light, i.color.a);
}
"#;

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Background: a light blue-grey.
const CLEAR: wgpu::Color = wgpu::Color {
    r: 0.80,
    g: 0.84,
    b: 0.88,
    a: 1.0,
};

enum Target {
    Surface {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    },
    Offscreen {
        texture: wgpu::Texture,
    },
}

pub struct WgpuViewport {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: Target,
    format: wgpu::TextureFormat,
    opaque_pipeline: wgpu::RenderPipeline,
    transparent_pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    depth: wgpu::TextureView,
    opaque: Option<(wgpu::Buffer, u32)>,
    transparent: Option<(wgpu::Buffer, u32)>,
    overlay: Option<(wgpu::Buffer, u32)>,
    size: (u32, u32),
    adapter: String,
    scene: RenderScene,
    highlight: std::collections::HashSet<reviera_core::ElementId>,
}

impl std::fmt::Debug for WgpuViewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuViewport")
            .field("adapter", &self.adapter)
            .field("size", &self.size)
            .finish()
    }
}

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

impl WgpuViewport {
    /// A viewport drawing into a native window.
    ///
    /// # Safety
    /// The handles must stay valid for the life of the viewport.
    pub unsafe fn for_window(
        display: RawDisplayHandle,
        window: RawWindowHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let instance = instance();
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display),
            raw_window_handle: window,
        };
        let surface =
            unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or("the window surface is not supported by the graphics adapter")?;
        surface.configure(&device, &config);
        let format = config.format;
        let info = adapter.get_info();
        Ok(Self::build(
            device,
            queue,
            Target::Surface { surface, config },
            format,
            (width.max(1), height.max(1)),
            info,
        ))
    }

    /// A viewport drawing into an offscreen texture.
    pub fn offscreen(width: u32, height: u32) -> Result<Self, String> {
        let instance = instance();
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| e.to_string())?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let texture = offscreen_texture(&device, format, width.max(1), height.max(1));
        let info = adapter.get_info();
        Ok(Self::build(
            device,
            queue,
            Target::Offscreen { texture },
            format,
            (width.max(1), height.max(1)),
            info,
        ))
    }

    fn build(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target: Target,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        info: wgpu::AdapterInfo,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |transparent: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if transparent { "transparent" } else { "opaque" }),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: transparent.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(!transparent),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque_pipeline = pipeline(false);
        let transparent_pipeline = pipeline(true);
        let depth = depth_view(&device, size);
        Self {
            device,
            queue,
            target,
            format,
            opaque_pipeline,
            transparent_pipeline,
            uniforms,
            bind_group,
            depth,
            opaque: None,
            transparent: None,
            size,
            adapter: format!("{} ({:?})", info.name, info.backend),
            scene: RenderScene::default(),
            highlight: Default::default(),
            overlay: None,
        }
    }

    /// The graphics adapter in use, for diagnostics.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Reads back an offscreen frame as tightly packed RGBA rows; `None` for window viewports.
    pub fn read_pixels(&self) -> Option<Vec<u8>> {
        let Target::Offscreen { texture } = &self.target else {
            return None;
        };
        let (w, h) = self.size;
        let row = w * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let data = slice.get_mapped_range().ok()?;
        let mut out = Vec::with_capacity((row * h) as usize);
        for r in 0..h {
            let start = (r * padded) as usize;
            out.extend_from_slice(&data[start..start + row as usize]);
        }
        Some(out)
    }
}

fn offscreen_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn depth_view(device: &wgpu::Device, (w, h): (u32, u32)) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
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

/// Expands indexed meshes into flat-shaded triangles, split by transparency; highlighted elements
/// are drawn opaque in the highlight colour.
fn vertices(
    meshes: &[reviera_render::SceneMesh],
    highlight: &std::collections::HashSet<reviera_core::ElementId>,
) -> (Vec<Vertex>, Vec<Vertex>) {
    let (mut opaque, mut transparent) = (Vec::new(), Vec::new());
    for m in meshes {
        let lit = m.element.is_some_and(|e| highlight.contains(&e));
        let color = if lit {
            reviera_render::HIGHLIGHT
        } else {
            m.color
        };
        let out = if color[3] < 0.99 {
            &mut transparent
        } else {
            &mut opaque
        };
        for tri in m.indices.as_chunks::<3>().0 {
            let p: Vec<glam::Vec3> = tri
                .iter()
                .map(|&i| glam::Vec3::from(m.positions[i as usize]))
                .collect();
            let normal = (p[1] - p[0])
                .cross(p[2] - p[0])
                .normalize_or_zero()
                .to_array();
            for q in p {
                out.push(Vertex {
                    position: q.to_array(),
                    normal,
                    color,
                });
            }
        }
    }
    (opaque, transparent)
}

impl RenderBackend for WgpuViewport {
    fn name(&self) -> &str {
        "wgpu"
    }

    fn set_scene(&mut self, scene: &RenderScene) {
        self.scene = scene.clone();
        self.upload();
    }

    fn set_highlight(&mut self, elements: &[reviera_core::ElementId]) {
        self.highlight = elements.iter().copied().collect();
        self.upload();
    }

    fn set_overlay(&mut self, meshes: &[reviera_render::SceneMesh]) {
        let (mut opaque, transparent) = vertices(meshes, &Default::default());
        opaque.extend(transparent);
        self.overlay = self.buffer(&opaque);
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.resize_target(width, height);
    }

    fn render(&mut self, camera: &Camera) -> Result<(), String> {
        self.draw(camera)
    }
}

impl WgpuViewport {
    fn buffer(&self, v: &[Vertex]) -> Option<(wgpu::Buffer, u32)> {
        (!v.is_empty()).then(|| {
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("scene"),
                    contents: bytemuck::cast_slice(v),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            (buffer, v.len() as u32)
        })
    }

    fn upload(&mut self) {
        let (opaque, transparent) = vertices(&self.scene.meshes, &self.highlight);
        self.opaque = self.buffer(&opaque);
        self.transparent = self.buffer(&transparent);
    }

    fn resize_target(&mut self, width: u32, height: u32) {
        let size = (width.max(1), height.max(1));
        if size == self.size {
            return;
        }
        self.size = size;
        match &mut self.target {
            Target::Surface { surface, config } => {
                config.width = size.0;
                config.height = size.1;
                surface.configure(&self.device, config);
            }
            Target::Offscreen { texture } => {
                *texture = offscreen_texture(&self.device, self.format, size.0, size.1)
            }
        }
        self.depth = depth_view(&self.device, size);
    }

    fn draw(&mut self, camera: &Camera) -> Result<(), String> {
        let aspect = self.size.0 as f32 / self.size.1 as f32;
        let u = Uniforms {
            view_proj: camera.view_proj(aspect).to_cols_array_2d(),
            eye: camera.eye().extend(1.0).to_array(),
        };
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));

        let frame = match &self.target {
            Target::Surface { surface, .. } => Some(match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t)
                | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                other => return Err(format!("no frame to draw into: {other:?}")),
            }),
            Target::Offscreen { .. } => None,
        };
        let view = match (&frame, &self.target) {
            (Some(f), _) => f.texture.create_view(&Default::default()),
            (None, Target::Offscreen { texture }) => texture.create_view(&Default::default()),
            (None, Target::Surface { .. }) => unreachable!(),
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            for (pipeline, mesh) in [
                (&self.opaque_pipeline, &self.opaque),
                (&self.transparent_pipeline, &self.transparent),
                (&self.transparent_pipeline, &self.overlay),
            ] {
                if let Some((buffer, count)) = mesh {
                    pass.set_pipeline(pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
        }
        self.queue.submit([encoder.finish()]);
        if let Some(f) = frame {
            self.queue.present(f);
        }
        Ok(())
    }
}
