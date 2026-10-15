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

/// The latest graphics error not caught where it happened. `wgpu` aborts on these by default; the
/// viewport reports them instead (the next frame returns it) so a driver or validation error does
/// not close the application.
static UNCAPTURED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Requests a device whose uncaptured errors are recorded, not fatal.
fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| e.to_string())?;
    device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
        eprintln!("graphics error: {e}");
        if let Ok(mut last) = UNCAPTURED.lock() {
            *last = Some(e.to_string());
        }
    }));
    Ok((device, queue))
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4, 2 => Float32x4];
const LINE_ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32];

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct LineInstance {
    a: [f32; 3],
    b: [f32; 3],
    color: [f32; 4],
    /// Paper millimetres.
    width: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    /// The face normal, and 1 in `w` when the face belongs to a closed, outward-wound solid
    /// whose inside may be shown as a cut face.
    normal: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    /// Viewport width and height in pixels, and pixels per paper millimetre (line widths).
    screen: [f32; 4],
    /// Clipping and plan style: top and bottom elevations kept, lightening toward white.
    clip: [f32; 4],
    /// The number of clip planes, and 1 to cap cut solids.
    counts: [f32; 4],
    /// Clip planes `[n, d]`: a point is kept when `n · p >= d` (DOC-VIEW-008, DOC-VIEW-014).
    planes: [[f32; 4]; PLANES],
}

/// Clip plane slots in the uniforms.
const PLANES: usize = 16;

/// No clipping, no lightening.
const NO_CLIP: [f32; 4] = [1e30, -1e30, 0.0, 0.0];
/// How far plan projection is lightened toward white.
const PLAN_LIGHTEN: f32 = 0.55;

const SHADER: &str = r#"
struct U {
    view_proj: mat4x4<f32>, eye: vec4<f32>, screen: vec4<f32>, clip: vec4<f32>,
    counts: vec4<f32>, planes: array<vec4<f32>, 16>,
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
    @location(3) solid: f32,
};

@vertex
fn vs(@location(0) p: vec3<f32>, @location(1) n: vec4<f32>, @location(2) c: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = u.view_proj * vec4<f32>(p, 1.0);
    o.normal = n.xyz;
    o.color = c;
    o.world = p;
    o.solid = n.w;
    return o;
}

// Thick lines: each instance is a segment expanded into a screen-space quad (two triangles),
// `width` paper millimetres wide (at least one pixel), drawn a little toward the camera so it wins
// over the faces it lies on.
@vertex
fn vs_thick(
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) c: vec4<f32>,
    @location(3) width: f32,
) -> VOut {
    let ends = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    let sides = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let end = ends[vi];
    let side = sides[vi];
    let ca = u.view_proj * vec4<f32>(a, 1.0);
    let cb = u.view_proj * vec4<f32>(b, 1.0);
    let px = max(u.screen.xy, vec2<f32>(1.0, 1.0));
    var dir = (cb.xy / cb.w - ca.xy / ca.w) * px;
    if (length(dir) < 1e-6) {
        dir = vec2<f32>(1.0, 0.0);
    }
    dir = normalize(dir);
    let normal = vec2<f32>(-dir.y, dir.x);
    let w = max(1.0, width * u.screen.z);
    // Half the width each side and half past each end, in clip units per pixel (2 / px).
    let offset = (normal * side + dir * (end * 2.0 - 1.0)) * w / px;
    let base = select(ca, cb, end > 0.5);
    var o: VOut;
    o.pos = vec4<f32>(base.xy + offset * base.w, base.z - 2e-6 * base.w, base.w);
    o.normal = vec3<f32>(0.0, 0.0, 1.0);
    o.color = c;
    o.world = mix(a, b, end);
    o.solid = 0.0;
    return o;
}

@fragment
fn fs(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (i.world.z > u.clip.x || i.world.z < u.clip.y) {
        discard;
    }
    let count = u32(u.counts.x);
    for (var k = 0u; k < count; k = k + 1u) {
        let pl = u.planes[k];
        if (dot(pl.xyz, i.world) < pl.w - 1e-5) {
            discard;
        }
    }
    // Interim capping (DOC-HLR-006): where a clip plane opens a closed solid, its inside shows;
    // drawn flat and darker it reads as the cut face.
    if (count > 0u && u.counts.y > 0.5 && i.solid > 0.5 && !front) {
        return vec4<f32>(i.color.rgb * 0.3, i.color.a);
    }
    let n = normalize(i.normal);
    let v = normalize(u.eye.xyz - i.world);
    let key = normalize(vec3<f32>(0.4, 0.3, 0.85));
    // Two-sided: faces lit whichever way they point. `eye.w` 0 draws unlit (overlay labels).
    let lit = 0.25 + 0.40 * abs(dot(n, v)) + 0.35 * abs(dot(n, key));
    let light = select(1.0, lit, u.eye.w > 0.5);
    let rgb = mix(i.color.rgb * light, vec3<f32>(1.0, 1.0, 1.0), u.clip.z);
    return vec4<f32>(rgb, i.color.a);
}
"#;

const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Multisampling (anti-aliasing); 4 is supported for every renderable format in WebGPU.
const SAMPLES: u32 = 4;
/// Background of plan views: paper white.
const PAPER: wgpu::Color = wgpu::Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
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
    /// The multisampled colour target, resolved into the window or texture at the end.
    msaa: wgpu::TextureView,
    opaque: Option<(wgpu::Buffer, u32)>,
    transparent: Option<(wgpu::Buffer, u32)>,
    overlay: Option<(wgpu::Buffer, u32)>,
    overlay_lines: Option<(wgpu::Buffer, u32)>,
    view_lines: Option<(wgpu::Buffer, u32)>,
    gizmo_pipeline: wgpu::RenderPipeline,
    /// Uniforms of the gizmo's lit geometry and of its flat (clip-space) geometry.
    gizmo_uniforms: [(wgpu::Buffer, wgpu::BindGroup); 2],
    /// The gizmo: pixel rectangle, matrices, and vertex buffers (lit, flat).
    gizmo: Option<GizmoGpu>,
    /// Plan graphics: clipping range, cut poché and lines (DOC-VIEW-002, interim GPU drawing).
    plan: Option<PlanGpu>,
    line_pipeline: wgpu::RenderPipeline,
    /// Uniforms of unlit, unclipped plan graphics.
    plan_uniforms: (wgpu::Buffer, wgpu::BindGroup),
    size: (u32, u32),
    /// The window has no area (minimized): nothing is configured or drawn until it has again.
    hidden: bool,
    adapter: String,
    scene: RenderScene,
    /// Per scene mesh: whether it is a closed solid, and whether its winding is inward.
    solids: Vec<Option<bool>>,
    highlight: std::collections::HashSet<reviera_core::ElementId>,
    visibility: reviera_render::Visibility,
    clipping: reviera_render::Clipping,
}

impl std::fmt::Debug for WgpuViewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuViewport")
            .field("adapter", &self.adapter)
            .field("size", &self.size)
            .finish()
    }
}

struct PlanGpu {
    top: f32,
    bottom: f32,
    scale: f32,
    poche: Option<(wgpu::Buffer, u32)>,
    lines: Option<(wgpu::Buffer, u32)>,
    /// The graphics, kept to rebuild the poché with a new highlight.
    graphics: reviera_render::plan::PlanGraphics,
}

struct GizmoGpu {
    rect: [f32; 4],
    view_proj: glam::Mat4,
    eye: glam::Vec3,
    lit: Option<(wgpu::Buffer, u32)>,
    flat: Option<(wgpu::Buffer, u32)>,
}

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

/// A graphics device shared by every viewport of the application (one per view window).
#[derive(Clone)]
pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl std::fmt::Debug for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gpu")
            .field("adapter", &self.adapter.get_info().name)
            .finish()
    }
}

impl WgpuViewport {
    /// A viewport drawing into a native window, with its own graphics device.
    ///
    /// # Safety
    /// The handles must stay valid for the life of the viewport.
    pub unsafe fn for_window(
        display: RawDisplayHandle,
        window: RawWindowHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        unsafe { Self::for_window_shared(&mut None, display, window, width, height) }
    }

    /// A viewport drawing into a native window. The first viewport creates the graphics device in
    /// `gpu`, choosing an adapter that can present to its window; later ones share it.
    ///
    /// # Safety
    /// The handles must stay valid for the life of the viewport.
    pub unsafe fn for_window_shared(
        gpu: &mut Option<Gpu>,
        display: RawDisplayHandle,
        window: RawWindowHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let instance = gpu.as_ref().map_or_else(instance, |g| g.instance.clone());
        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display),
            raw_window_handle: window,
        };
        let surface =
            unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())?;
        let shared = match gpu {
            Some(g) => g.clone(),
            None => {
                let adapter =
                    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                        compatible_surface: Some(&surface),
                        ..Default::default()
                    }))
                    .map_err(|e| e.to_string())?;
                let (device, queue) = request_device(&adapter)?;
                gpu.insert(Gpu {
                    instance,
                    adapter,
                    device,
                    queue,
                })
                .clone()
            }
        };
        let config = surface
            .get_default_config(&shared.adapter, width.max(1), height.max(1))
            .ok_or("the window surface is not supported by the graphics adapter")?;
        // Configured when the window has an area (see `resize_target`).
        let hidden = width == 0 || height == 0;
        if !hidden {
            surface.configure(&shared.device, &config);
        }
        let format = config.format;
        let info = shared.adapter.get_info();
        let mut viewport = Self::build(
            shared.device,
            shared.queue,
            Target::Surface { surface, config },
            format,
            (width.max(1), height.max(1)),
            info,
        );
        viewport.hidden = hidden;
        Ok(viewport)
    }

    /// A viewport drawing into an offscreen texture.
    pub fn offscreen(width: u32, height: u32) -> Result<Self, String> {
        let instance = instance();
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| e.to_string())?;
        let (device, queue) = request_device(&adapter)?;
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
        // `gizmo`: blended like transparent geometry, but writing depth (the Orientation Cube
        // hides what is behind it, and fades as a whole).
        let pipeline = |transparent: bool, gizmo: bool, line: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if gizmo {
                    "gizmo"
                } else if transparent {
                    "transparent"
                } else {
                    "opaque"
                }),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if line { "vs_thick" } else { "vs" }),
                    compilation_options: Default::default(),
                    buffers: &[Some(if line {
                        wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<LineInstance>() as u64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &LINE_ATTRIBUTES,
                        }
                    } else {
                        wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &VERTEX_ATTRIBUTES,
                        }
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: (transparent || gizmo).then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(!transparent || gizmo),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque_pipeline = pipeline(false, false, false);
        let transparent_pipeline = pipeline(true, false, false);
        let gizmo_pipeline = pipeline(true, true, false);
        let line_pipeline = pipeline(false, false, true);
        let uniform_set = || {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gizmo uniforms"),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            (buffer, group)
        };
        let gizmo_uniforms = [uniform_set(), uniform_set()];
        let plan_uniforms = uniform_set();
        let depth = depth_view(&device, size);
        let msaa = msaa_view(&device, format, size);
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
            msaa,
            opaque: None,
            transparent: None,
            size,
            hidden: false,
            adapter: format!("{} ({:?})", info.name, info.backend),
            scene: RenderScene::default(),
            solids: Vec::new(),
            highlight: Default::default(),
            visibility: Default::default(),
            clipping: Default::default(),
            overlay: None,
            overlay_lines: None,
            view_lines: None,
            gizmo_pipeline,
            gizmo_uniforms,
            gizmo: None,
            plan: None,
            line_pipeline,
            plan_uniforms,
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

fn msaa_view(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    (w, h): (u32, u32),
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Line instances of line sets (one per segment).
fn line_vertices(lines: &[reviera_render::plan::Lines]) -> Vec<LineInstance> {
    lines
        .iter()
        .flat_map(|l| {
            l.segments.iter().map(move |s| LineInstance {
                a: s[0],
                b: s[1],
                color: l.color,
                width: l.width,
            })
        })
        .collect()
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
            sample_count: SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// Whether a mesh is a closed solid with consistent winding: `Some(flip)` (its winding is
/// inward when `flip`), `None` when it is open or inconsistent. Vertices are welded by position.
pub fn solid(m: &reviera_render::SceneMesh) -> Option<bool> {
    use std::collections::HashMap;
    let mut weld: HashMap<[i64; 3], u32> = HashMap::new();
    let ids: Vec<u32> = m
        .positions
        .iter()
        .map(|p| {
            let key = p.map(|v| (f64::from(v) * 1e5).round() as i64);
            let n = weld.len() as u32;
            *weld.entry(key).or_insert(n)
        })
        .collect();
    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    let mut volume = 0.0f64;
    for tri in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = tri.map(|k| ids[k as usize]);
        if a == b || b == c || a == c {
            continue;
        }
        for e in [(a, b), (b, c), (c, a)] {
            *edges.entry(e).or_default() += 1;
        }
        let [p, q, r] = tri.map(|k| glam::DVec3::from(m.positions[k as usize].map(f64::from)));
        volume += p.dot(q.cross(r));
    }
    let closed = !edges.is_empty()
        && edges
            .iter()
            .all(|(&(a, b), &n)| n == 1 && edges.get(&(b, a)) == Some(&1));
    (closed && volume.abs() > 1e-12).then_some(volume < 0.0)
}

/// Expands indexed meshes into flat-shaded triangles, split by transparency: highlighted
/// elements are drawn in the highlight colour, hidden ones are left out and translucent ones
/// fade (DOC-GFX-008). `solids` (per mesh, from [`solid`]) marks faces that can show a cut.
fn vertices(
    meshes: &[reviera_render::SceneMesh],
    solids: &[Option<bool>],
    highlight: &std::collections::HashSet<reviera_core::ElementId>,
    visibility: &reviera_render::Visibility,
) -> (Vec<Vertex>, Vec<Vertex>) {
    let (mut opaque, mut transparent) = (Vec::new(), Vec::new());
    for (k, m) in meshes.iter().enumerate() {
        let Some(alpha) = visibility.opacity(m.element) else {
            continue;
        };
        let lit = m.element.is_some_and(|e| highlight.contains(&e));
        let mut color = if lit {
            reviera_render::HIGHLIGHT
        } else {
            m.color
        };
        if reviera_render::is_virtual(&m.class) {
            match visibility.virtual_volumes {
                reviera_render::VirtualMode::Hidden => continue,
                reviera_render::VirtualMode::Translucent => {
                    // A selected door's clearances light up but stay see-through.
                    color[3] = m.color[3];
                }
                reviera_render::VirtualMode::Solid => color[3] = 1.0,
            }
        }
        color[3] *= alpha;
        let out = if color[3] < 0.99 {
            &mut transparent
        } else {
            &mut opaque
        };
        let solid = solids.get(k).copied().flatten();
        for tri in m.indices.as_chunks::<3>().0 {
            let mut p: Vec<glam::Vec3> = tri
                .iter()
                .map(|&i| glam::Vec3::from(m.positions[i as usize]))
                .collect();
            if solid == Some(true) {
                p.swap(1, 2);
            }
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            let normal = [n.x, n.y, n.z, f32::from(u8::from(solid.is_some()))];
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

/// [`vertices`] of geometry that is never cut, hidden or capped (overlays, gizmos, plan poché).
fn plain_vertices(
    meshes: &[reviera_render::SceneMesh],
    highlight: &std::collections::HashSet<reviera_core::ElementId>,
) -> Vec<Vertex> {
    let (mut v, t) = vertices(meshes, &[], highlight, &Default::default());
    v.extend(t);
    v
}

impl RenderBackend for WgpuViewport {
    fn name(&self) -> &str {
        "wgpu"
    }

    fn set_scene(&mut self, scene: &RenderScene) {
        self.scene = scene.clone();
        self.solids = self.scene.meshes.iter().map(solid).collect();
        self.upload();
    }

    fn set_view_lines(&mut self, lines: &[reviera_render::plan::Lines]) {
        let v = line_vertices(lines);
        self.view_lines = self.buffer(&v);
    }

    fn set_visibility(&mut self, visibility: &reviera_render::Visibility) {
        if *visibility != self.visibility {
            self.visibility = visibility.clone();
            self.upload();
        }
    }

    fn set_clipping(&mut self, clipping: &reviera_render::Clipping) {
        self.clipping = clipping.clone();
    }

    fn set_highlight(&mut self, elements: &[reviera_core::ElementId]) {
        self.highlight = elements.iter().copied().collect();
        self.upload();
        if let Some(g) = self.plan.take().map(|p| p.graphics) {
            self.set_plan(Some(g));
        }
    }

    fn set_overlay(&mut self, meshes: &[reviera_render::SceneMesh]) {
        let v = plain_vertices(meshes, &Default::default());
        self.overlay = self.buffer(&v);
    }

    fn set_overlay_lines(&mut self, lines: &[reviera_render::plan::Lines]) {
        let v = line_vertices(lines);
        self.overlay_lines = self.buffer(&v);
    }

    fn set_plan(&mut self, plan: Option<reviera_render::plan::PlanGraphics>) {
        self.plan = plan.map(|g| {
            let poche = plain_vertices(&g.poche, &self.highlight);
            let lines = line_vertices(&g.lines);
            PlanGpu {
                top: g.top,
                bottom: g.bottom,
                scale: g.scale,
                poche: self.buffer(&poche),
                lines: self.buffer(&lines),
                graphics: g,
            }
        });
    }

    fn set_gizmo(&mut self, gizmo: Option<reviera_render::Gizmo>) {
        self.gizmo = gizmo.map(|g| {
            let expand = |meshes: &[reviera_render::SceneMesh]| {
                self.buffer(&plain_vertices(meshes, &Default::default()))
            };
            GizmoGpu {
                rect: g.rect,
                view_proj: g.view_proj,
                eye: g.eye,
                lit: expand(&g.meshes),
                flat: expand(&g.flat),
            }
        });
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.resize_target(width, height);
    }

    fn render(&mut self, camera: &Camera) -> Result<(), String> {
        self.draw(camera)
    }
}

impl WgpuViewport {
    fn buffer<T: Pod>(&self, v: &[T]) -> Option<(wgpu::Buffer, u32)> {
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
        let (opaque, transparent) = vertices(
            &self.scene.meshes,
            &self.solids,
            &self.highlight,
            &self.visibility,
        );
        self.opaque = self.buffer(&opaque);
        self.transparent = self.buffer(&transparent);
    }

    /// Draws the gizmo into its rectangle, over the scene, with a fresh depth buffer.
    fn draw_gizmo(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        g: &GizmoGpu,
    ) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let [x, y, rw, rh] = g.rect;
        let (x0, y0) = (x.clamp(0.0, w), y.clamp(0.0, h));
        let (x1, y1) = ((x + rw).clamp(0.0, w), (y + rh).clamp(0.0, h));
        if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
            return;
        }
        let write = |k: usize, view_proj: glam::Mat4, eye: [f32; 4]| {
            let u = Uniforms {
                view_proj: view_proj.to_cols_array_2d(),
                eye,
                screen: [0.0; 4],
                clip: NO_CLIP,
                counts: [0.0; 4],
                planes: [[0.0; 4]; PLANES],
            };
            self.queue
                .write_buffer(&self.gizmo_uniforms[k].0, 0, bytemuck::bytes_of(&u));
        };
        write(0, g.view_proj, g.eye.extend(1.0).to_array());
        write(1, glam::Mat4::IDENTITY, [0.0, 0.0, 1.0, 0.0]);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gizmo"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.msaa,
                depth_slice: None,
                resolve_target: Some(view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
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
            ..Default::default()
        });
        // The viewport is the gizmo's full rectangle (it may extend past the view's edges); the
        // scissor keeps drawing inside the view.
        pass.set_viewport(x, y, rw, rh, 0.0, 1.0);
        pass.set_scissor_rect(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
        pass.set_pipeline(&self.gizmo_pipeline);
        for (k, mesh) in [(0, &g.lit), (1, &g.flat)] {
            if let Some((buffer, count)) = mesh {
                pass.set_bind_group(0, &self.gizmo_uniforms[k].1, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..*count, 0..1);
            }
        }
        // The pass ends by resolving the whole view into the frame. On the GL backend that is a
        // framebuffer blit, which the scissor clips: left at the gizmo's rectangle, only the gizmo
        // would reach the screen (the rest stays black).
        pass.set_scissor_rect(0, 0, self.size.0, self.size.1);
    }

    fn resize_target(&mut self, width: u32, height: u32) {
        // A minimized window has no area. Configuring its surface (at any size) is an error on
        // Vulkan and Direct3D, so it is left as it was until the window is restored.
        self.hidden = width == 0 || height == 0;
        let size = (width.max(1), height.max(1));
        if self.hidden || size == self.size {
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
        self.msaa = msaa_view(&self.device, self.format, size);
    }

    fn draw(&mut self, camera: &Camera) -> Result<(), String> {
        if let Some(e) = UNCAPTURED.lock().ok().and_then(|mut e| e.take()) {
            return Err(format!("Graphics error: {e}"));
        }
        if self.hidden {
            return Ok(());
        }
        let aspect = self.size.0 as f32 / self.size.1 as f32;
        let view_proj = camera.view_proj(aspect).to_cols_array_2d();
        let clip = match &self.plan {
            Some(p) => [p.top + 1e-4, p.bottom, PLAN_LIGHTEN, 0.0],
            None => NO_CLIP,
        };
        // Pixels per paper millimetre: pixels per metre at the target, times metres per paper
        // millimetre at the drawing scale (1:100 when there is no plan).
        let scale = self.plan.as_ref().map_or(100.0, |p| p.scale);
        let px_per_m = self.size.1 as f32 / (2.0 * camera.half_height()).max(1e-6);
        let screen = [
            self.size.0 as f32,
            self.size.1 as f32,
            px_per_m * scale / 1000.0,
            0.0,
        ];
        // Clip planes apply to 3D views only; plans have their own range.
        let mut planes = [[0.0; 4]; PLANES];
        let mut count = 0;
        if self.plan.is_none() {
            for (slot, p) in planes.iter_mut().zip(&self.clipping.planes) {
                *slot = *p;
                count += 1;
            }
        }
        let u = Uniforms {
            view_proj,
            eye: camera.eye().extend(1.0).to_array(),
            screen,
            clip,
            counts: [
                count as f32,
                f32::from(u8::from(self.clipping.caps)),
                0.0,
                0.0,
            ],
            planes,
        };
        let flat = Uniforms {
            view_proj,
            eye: [0.0, 0.0, 1.0, 0.0],
            screen,
            clip: NO_CLIP,
            counts: [0.0; 4],
            planes: [[0.0; 4]; PLANES],
        };
        self.queue
            .write_buffer(&self.plan_uniforms.0, 0, bytemuck::bytes_of(&flat));
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));

        let frame = match &self.target {
            Target::Surface { surface, .. } => Some(match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t)
                | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                // The window changed under the surface (restored, moved to another screen):
                // configured again, drawn on the next paint.
                wgpu::CurrentSurfaceTexture::Outdated => {
                    if let Target::Surface { surface, config } = &self.target {
                        surface.configure(&self.device, config);
                    }
                    return Ok(());
                }
                // Minimized or covered, or slow: this frame is skipped.
                wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                    return Ok(());
                }
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
            // Resolved into the frame here, or after the gizmo pass when there is one.
            let resolve = self.gizmo.is_none().then_some(&view);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if self.plan.is_some() { PAPER } else { CLEAR }),
                        store: if resolve.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
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
            // A ceiling plan looks up: the model above its cut would hide the cut from the
            // top-down camera, so only its plan graphics are drawn.
            let look_up = self.plan.as_ref().is_some_and(|p| p.graphics.look_up);
            for (pipeline, mesh, model) in [
                (&self.opaque_pipeline, &self.opaque, true),
                (&self.transparent_pipeline, &self.transparent, true),
                (&self.transparent_pipeline, &self.overlay, false),
            ] {
                if model && look_up {
                    continue;
                }
                if let Some((buffer, count)) = mesh {
                    pass.set_pipeline(pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
            // Plan poché and lines, then tool preview lines: unlit, unclipped.
            pass.set_bind_group(0, &self.plan_uniforms.1, &[]);
            let none = None;
            let (poche, lines) = match &self.plan {
                Some(p) => (&p.poche, &p.lines),
                None => (&none, &none),
            };
            {
                for (pipeline, mesh) in [
                    (&self.opaque_pipeline, poche),
                    (&self.line_pipeline, lines),
                    (&self.line_pipeline, &self.overlay_lines),
                    (&self.line_pipeline, &self.view_lines),
                ] {
                    if let Some((buffer, count)) = mesh {
                        pass.set_pipeline(pipeline);
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        if std::ptr::eq(pipeline, &self.line_pipeline) {
                            // Six vertices (two triangles) per segment instance.
                            pass.draw(0..6, 0..*count);
                        } else {
                            pass.draw(0..*count, 0..1);
                        }
                    }
                }
            }
        }
        if let Some(g) = &self.gizmo {
            self.draw_gizmo(&mut encoder, &view, g);
        }
        self.queue.submit([encoder.finish()]);
        if let Some(f) = frame {
            self.queue.present(f);
        }
        Ok(())
    }
}
