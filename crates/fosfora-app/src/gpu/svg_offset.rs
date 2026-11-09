//! Runtime SVG geometry and cached distance fields for the SVG Offset layer.
//!
//! IDs are painter-ordered: 0 is background, shape N is N+1. usvg normalizes
//! curves, primitives, transforms and uses; tiny-skia rasterizes fills and strokes.

use bytemuck::{Pod, Zeroable};
use tiny_skia::{FillRule, Paint, Path, Pixmap, Transform};

pub const DEFAULT_SVG: &str = include_str!("../../../../assets/svg/ornament.svg");
const MAX_SHAPES: usize = 65_535;

/// Storage-buffer record. Index zero is a dummy background entry.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
pub struct ShapeMeta {
    pub color: [f32; 4],
    /// Centroid x/y (normalized canvas), group index, visible area (pixels).
    pub geometry: [f32; 4],
    pub bounds: [f32; 4],
}

struct Shape {
    parts: Vec<(Path, FillRule)>,
    meta: ShapeMeta,
}

pub struct SvgDocument {
    shapes: Vec<Shape>,
    size: [f32; 2],
}

impl SvgDocument {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
            return Err("SVG exceeds the 16 MiB geometry limit".into());
        }
        Self::parse(&std::fs::read(path).map_err(|e| e.to_string())?)
    }

    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let tree = usvg::Tree::from_data(data, &usvg::Options::default())
            .map_err(|e| format!("Invalid SVG: {e}"))?;
        let mut doc = Self {
            shapes: Vec::new(),
            size: [tree.size().width(), tree.size().height()],
        };
        doc.walk(tree.root(), 0, &mut 0, 1.0)?;
        if doc.shapes.is_empty() {
            return Err("SVG contains no visible vector fills or strokes".into());
        }
        Ok(doc)
    }

    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    fn walk(
        &mut self,
        group: &usvg::Group,
        group_id: u32,
        next_group: &mut u32,
        opacity: f32,
    ) -> Result<(), String> {
        let opacity = opacity * group.opacity().get();
        if opacity == 0.0 {
            return Ok(());
        }
        for node in group.children() {
            match node {
                usvg::Node::Group(child) => {
                    *next_group += 1;
                    self.walk(child, *next_group, next_group, opacity)?;
                }
                usvg::Node::Path(path) if path.is_visible() => {
                    let mut parts = Vec::new();
                    let mut color = [1.0; 4];
                    if let Some(fill) = path.fill().filter(|f| f.opacity().get() > 0.0) {
                        if let Some(data) = path.data().clone().transform(path.abs_transform()) {
                            let rule = match fill.rule() {
                                usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                                usvg::FillRule::NonZero => FillRule::Winding,
                            };
                            parts.push((data, rule));
                        }
                        color = paint_color(fill.paint(), fill.opacity().get() * opacity);
                    }
                    if let Some(stroke) = path.stroke().filter(|s| s.opacity().get() > 0.0) {
                        if let Some(data) = path
                            .data()
                            .stroke(&stroke.to_tiny_skia(), 1.0)
                            .and_then(|p| p.transform(path.abs_transform()))
                        {
                            parts.push((data, FillRule::Winding));
                        }
                        if path.fill().is_none() {
                            color = paint_color(stroke.paint(), stroke.opacity().get() * opacity);
                        }
                    }
                    if !parts.is_empty() {
                        if self.shapes.len() == MAX_SHAPES {
                            return Err("SVG exceeds the 65535-shape limit".into());
                        }
                        self.shapes.push(Shape {
                            parts,
                            meta: ShapeMeta {
                                color,
                                geometry: [0.0, 0.0, group_id as f32, 0.0],
                                ..ShapeMeta::default()
                            },
                        });
                    }
                }
                // Raster images and text without vector outlines are not shapes.
                _ => {}
            }
        }
        Ok(())
    }

    /// Fit the SVG viewport, preserving aspect and one empty border pixel. IDs
    /// are opaque RGB with AA off, not blended colors (including >255 shapes).
    pub fn rasterize(&self, width: u32, height: u32) -> Result<IdMask, String> {
        if width < 3 || height < 3 || width > 8192 || height > 8192 {
            return Err("SVG mask dimensions must be in 3..=8192".into());
        }
        let mut pixmap = Pixmap::new(width, height).ok_or("Cannot allocate SVG mask")?;
        let scale = ((width - 2) as f32 / self.size[0]).min((height - 2) as f32 / self.size[1]);
        let transform = Transform::from_row(
            scale,
            0.0,
            0.0,
            scale,
            (width as f32 - self.size[0] * scale) * 0.5,
            (height as f32 - self.size[1] * scale) * 0.5,
        );
        let mut metadata = vec![ShapeMeta::default()];
        for (index, shape) in self.shapes.iter().enumerate() {
            let id = index as u32 + 1;
            let mut paint = Paint {
                anti_alias: false,
                ..Paint::default()
            };
            paint.set_color_rgba8(id as u8, (id >> 8) as u8, (id >> 16) as u8, 255);
            for (path, rule) in &shape.parts {
                pixmap.fill_path(path, &paint, *rule, transform, None);
            }
            let mut meta = shape.meta;
            meta.bounds = [1.0, 1.0, 0.0, 0.0];
            metadata.push(meta);
        }
        let ids: Vec<u32> = pixmap
            .pixels()
            .iter()
            .map(|p| u32::from(p.red()) | (u32::from(p.green()) << 8) | (u32::from(p.blue()) << 16))
            .collect();
        for (i, &id) in ids.iter().enumerate().filter(|(_, id)| **id != 0) {
            let x = (i as u32 % width) as f32 / width as f32;
            let y = (i as u32 / width) as f32 / height as f32;
            let m = &mut metadata[id as usize];
            m.geometry[0] += x;
            m.geometry[1] += y;
            m.geometry[3] += 1.0;
            m.bounds[0] = m.bounds[0].min(x);
            m.bounds[1] = m.bounds[1].min(y);
            m.bounds[2] = m.bounds[2].max(x);
            m.bounds[3] = m.bounds[3].max(y);
        }
        for m in &mut metadata[1..] {
            let area = m.geometry[3].max(1.0);
            m.geometry[0] /= area;
            m.geometry[1] /= area;
        }
        Ok(IdMask {
            width,
            height,
            ids,
            metadata,
        })
    }
}

fn paint_color(paint: &usvg::Paint, opacity: f32) -> [f32; 4] {
    // One representative original color per shape; gradients use the first stop.
    // Geometry deliberately does not describe SVG filter/clip compositing.
    let color = match paint {
        usvg::Paint::Color(c) => *c,
        usvg::Paint::LinearGradient(g) => g.stops()[0].color(),
        usvg::Paint::RadialGradient(g) => g.stops()[0].color(),
        usvg::Paint::Pattern(_) => usvg::Color::white(),
    };
    [
        color.red as f32 / 255.0,
        color.green as f32 / 255.0,
        color.blue as f32 / 255.0,
        opacity,
    ]
}

pub struct IdMask {
    pub width: u32,
    pub height: u32,
    pub ids: Vec<u32>,
    pub metadata: Vec<ShapeMeta>,
}

/// Non-bindable geometry/transform/keyer controls, persisted with the layer.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SvgOffsetSettings {
    /// Resolution divisor: 1 (full), 2 (half), 4 (quarter).
    pub quality: u32,
    pub offset: [f32; 2],
    pub rotation: f32,
    /// 0 alpha, 1 black, 2 white.
    pub background: u32,
    pub key_width: f32,
    pub key_phase: f32,
    pub key_soft: f32,
    pub key_repeat: f32,
}
impl Default for SvgOffsetSettings {
    fn default() -> Self {
        Self {
            quality: 1,
            offset: [0.0; 2],
            rotation: 0.0,
            background: 0,
            key_width: 1.0,
            key_phase: 0.0,
            key_soft: 0.05,
            key_repeat: 1.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawUniforms {
    params: [[f32; 4]; 4],
    geometry: [f32; 4],
    transform: [f32; 4],
    key: [f32; 4],
}

/// Cached field resources. Keeping the mask and both JFA targets makes their
/// lifetime explicit; no distance compute is encoded during ordinary frames.
struct FieldGpu {
    _mask: wgpu::Texture,
    _ping: [wgpu::Texture; 2],
    pub view: wgpu::TextureView,
    _field: wgpu::Texture,
    metadata: wgpu::Buffer,
    size: [u32; 2],
}

fn texture(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("svg-offset-field"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn buffer(device: &wgpu::Device, data: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("svg-offset-buffer"),
        contents: data,
        usage,
    })
}

impl FieldGpu {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, mask: IdMask) -> Self {
        use wgpu::{
            BindingResource as R, BufferUsages as B, TextureFormat as F, TextureUsages as T,
        };
        let size = [mask.width, mask.height];
        let id_texture = texture(device, size, F::R32Uint, T::TEXTURE_BINDING | T::COPY_DST);
        queue.write_texture(
            id_texture.as_image_copy(),
            bytemuck::cast_slice(&mask.ids),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(mask.width * 4),
                rows_per_image: Some(mask.height),
            },
            id_texture.size(),
        );
        let id_view = id_texture.create_view(&Default::default());
        let ping = std::array::from_fn(|_| {
            texture(
                device,
                size,
                F::Rg32Float,
                T::TEXTURE_BINDING | T::STORAGE_BINDING,
            )
        });
        let views: [wgpu::TextureView; 2] =
            std::array::from_fn(|i| ping[i].create_view(&Default::default()));
        let field = texture(
            device,
            size,
            F::Rgba32Float,
            T::TEXTURE_BINDING | T::STORAGE_BINDING | T::COPY_SRC,
        );
        let view = field.create_view(&Default::default());
        let metadata = buffer(device, bytemuck::cast_slice(&mask.metadata), B::STORAGE);
        let radii = buffer(
            device,
            bytemuck::cast_slice(&vec![0u32; mask.metadata.len()]),
            B::STORAGE,
        );
        let interior = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("svg-offset-interior"),
            size: u64::from(mask.width) * u64::from(mask.height) * 4,
            usage: B::STORAGE,
            mapped_at_creation: false,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("svg-offset-jfa"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../../../../assets/shaders/builtin/svg_offset_field.wgsl").into(),
            ),
        });
        // An explicit nonfilterable float layout works without FLOAT32_FILTERABLE.
        let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..7)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                count: None,
                ty: match binding {
                    0 => wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    1 | 2 => wgpu::BindingType::Texture {
                        sample_type: if binding == 1 {
                            wgpu::TextureSampleType::Uint
                        } else {
                            wgpu::TextureSampleType::Float { filterable: false }
                        },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    3 | 4 => wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: if binding == 3 {
                            F::Rg32Float
                        } else {
                            F::Rgba32Float
                        },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    _ => wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                },
            })
            .collect();
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("svg-offset-compute"),
            entries: &entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipelines: Vec<_> = ["seed", "jump", "reduce", "finish"]
            .iter()
            .map(|entry| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
            })
            .collect();
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut dispatch = |pipeline: usize, source: usize, step: u32, inside: u32| {
            // Each dispatch owns its constants. Queue-writing one uniform repeatedly
            // before submit would give EVERY jump the last step (1), not the JFA ladder.
            let config = buffer(
                device,
                bytemuck::cast_slice(&[size[0], size[1], step, inside]),
                B::UNIFORM,
            );
            let resources = [
                config.as_entire_binding(),
                R::TextureView(&id_view),
                R::TextureView(&views[source]),
                R::TextureView(&views[1 - source]),
                R::TextureView(&view),
                radii.as_entire_binding(),
                interior.as_entire_binding(),
            ];
            let entries: Vec<_> = resources
                .into_iter()
                .enumerate()
                .map(|(binding, resource)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource,
                })
                .collect();
            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &bgl,
                entries: &entries,
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("svg-offset-jfa"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipelines[pipeline]);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        };
        for inside in [1, 0] {
            dispatch(0, 1, 0, inside); // seed -> ping[0]
            let mut source = 0;
            let mut step = size[0].max(size[1]).next_power_of_two() / 2;
            while step > 0 {
                dispatch(1, source, step, inside);
                source = 1 - source;
                step /= 2;
            }
            dispatch(1, source, 1, inside);
            source = 1 - source; // JFA+1
            dispatch(if inside == 1 { 2 } else { 3 }, source, 0, inside);
        }
        queue.submit([encoder.finish()]);
        Self {
            _mask: id_texture,
            _ping: ping,
            _field: field,
            view,
            metadata,
            size,
        }
    }
}

pub struct SvgOffsetSim {
    document: SvgDocument,
    pub path: Option<String>,
    pub settings: SvgOffsetSettings,
    size: [u32; 2],
    field: FieldGpu,
    pub sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
}

impl SvgOffsetSim {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let document = SvgDocument::parse(DEFAULT_SVG.as_bytes())?;
        let field = FieldGpu::new(
            device,
            queue,
            document.rasterize(width.max(3), height.max(3))?,
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("svg-offset-draw"),
            size: std::mem::size_of::<DrawUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let source = format!(
            "{}\n{}",
            super::fullscreen_quad::FULLSCREEN_TRIANGLE_VS_WITH_UV,
            include_str!("../../../../assets/shaders/builtin/svg_offset_draw.wgsl")
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("svg-offset-draw"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        // Auto layouts default float textures to filterable, so declare this one.
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("svg-offset-draw"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(format.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let bind_group = Self::draw_group(device, &bgl, &uniforms, &field);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        Ok(Self {
            document,
            path: None,
            settings: SvgOffsetSettings::default(),
            size: [width, height],
            field,
            sampler,
            pipeline,
            bind_group,
            uniforms,
        })
    }

    fn draw_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniforms: &wgpu::Buffer,
        field: &FieldGpu,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&field.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: field.metadata.as_entire_binding(),
                },
            ],
        })
    }
    pub fn output_size(&self) -> [u32; 2] {
        self.size
    }
    pub fn sdf(&self) -> (&wgpu::TextureView, &wgpu::Sampler) {
        (&self.field.view, &self.sampler)
    }
    pub fn shape_count(&self) -> usize {
        self.document.shape_count()
    }

    pub fn reload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        path: Option<&str>,
        settings: SvgOffsetSettings,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let doc = match path {
            Some(path) => SvgDocument::load(std::path::Path::new(path))?,
            None => SvgDocument::parse(DEFAULT_SVG.as_bytes())?,
        };
        let divisor = match settings.quality {
            2 => 2,
            4 => 4,
            _ => 1,
        };
        let mask = doc.rasterize(
            (width / divisor).clamp(3, 8192),
            (height / divisor).clamp(3, 8192),
        )?;
        self.field = FieldGpu::new(device, queue, mask);
        self.bind_group = Self::draw_group(
            device,
            &self.pipeline.get_bind_group_layout(0),
            &self.uniforms,
            &self.field,
        );
        self.document = doc;
        self.path = path.map(str::to_owned);
        self.settings = settings;
        self.size = [width, height];
        Ok(())
    }
    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let divisor = match self.settings.quality {
            2 => 2,
            4 => 4,
            _ => 1,
        };
        self.field = FieldGpu::new(
            device,
            queue,
            self.document.rasterize(
                (width / divisor).clamp(3, 8192),
                (height / divisor).clamp(3, 8192),
            )?,
        );
        self.bind_group = Self::draw_group(
            device,
            &self.pipeline.get_bind_group_layout(0),
            &self.uniforms,
            &self.field,
        );
        self.size = [width, height];
        Ok(())
    }
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        params: &[f32; 16],
        target: &wgpu::TextureView,
    ) {
        let u = DrawUniforms {
            params: std::array::from_fn(|i| std::array::from_fn(|j| params[i * 4 + j])),
            geometry: [
                self.field.size[0] as f32,
                self.field.size[1] as f32,
                self.shape_count() as f32,
                self.settings.background as f32,
            ],
            transform: [
                self.settings.offset[0],
                self.settings.offset[1],
                self.settings.rotation.to_radians(),
                self.size[0] as f32 / self.size[1].max(1) as f32,
            ],
            key: [
                self.settings.key_width,
                self.settings.key_phase,
                self.settings.key_soft,
                self.settings.key_repeat,
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("svg-offset-draw"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn param_defaults_and_old_presets() {
        let effect: crate::effect::format::PfxEffect =
            serde_json::from_str(include_str!("../../../../assets/effects/svg_offset.pfx"))
                .unwrap();
        assert_eq!(effect.category, "pattern");
        assert!(effect.svg_offset.is_some());
        let mut store = crate::params::ParamStore::new();
        store.load_from_defs(&effect.inputs);
        let mut packed = store.pack_to_buffer();
        assert_eq!(
            &packed[..13],
            &[
                12.0, 0.32, 1.0, 0.15, 0.0, 0.0, 1.0, 0.0, 0.35, 1.0, 0.035, 0.0, 1.0
            ]
        );
        let slots = crate::effect::rates::layout(&effect.inputs, &effect.rates).unwrap();
        assert_eq!(slots.iter().map(|s| s.dst).collect::<Vec<_>>(), [13, 14]);
        for name in ["mode", "color_mode", "order"] {
            assert!(
                effect.inputs.iter().any(
                    |d| matches!(d, crate::params::ParamDef::Int { name: n, .. } if n == name)
                )
            );
        }
        store.set("mode", crate::params::ParamValue::Float(0.7));
        packed = store.pack_to_buffer();
        assert_eq!(packed[5], 1.0);
        let old: crate::preset::store::LayerPreset =
            serde_json::from_str(r#"{"effect_name":"SVG Offset"}"#).unwrap();
        assert!(old.svg_offset_path.is_none());
        let mut saved = old;
        saved.svg_offset_path = Some("/tmp/art.svg".into());
        saved.svg_offset_settings = Some(SvgOffsetSettings {
            quality: 2,
            ..Default::default()
        });
        let restored: crate::preset::store::LayerPreset =
            serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(restored.svg_offset_path, saved.svg_offset_path);
        assert_eq!(restored.svg_offset_settings, saved.svg_offset_settings);
    }
    #[test]
    fn shaders_validate() {
        for source in [
            include_str!("../../../../assets/shaders/builtin/svg_offset_field.wgsl").to_string(),
            format!(
                "{}\n{}",
                super::super::fullscreen_quad::FULLSCREEN_TRIANGLE_VS_WITH_UV,
                include_str!("../../../../assets/shaders/builtin/svg_offset_draw.wgsl")
            ),
        ] {
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap();
        }
    }
    #[test]
    #[ignore = "requires a GPU"]
    fn gpu_field_and_draw() {
        let _guard = crate::gpu::test_gpu::gpu_guard();
        let (device, queue) = crate::gpu::test_gpu::test_gpu();
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut sim =
            SvgOffsetSim::new(&device, &queue, wgpu::TextureFormat::Rgba16Float, 64, 64).unwrap();
        let target = texture(
            &device,
            [64, 64],
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        sim.render(
            &mut encoder,
            &queue,
            &[
                12.0, 0.32, 1.0, 0.15, 0.0, 0.0, 1.0, 0.0, 0.35, 1.0, 0.035, 0.0, 1.0, 0.0, 0.0,
                0.0,
            ],
            &target.create_view(&Default::default()),
        );
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert!(pollster::block_on(device.pop_error_scope()).is_none());
        // Exercise the production special-input path too: its Rgba32Float binding
        // must be nonfiltering, including before attachment and after resize.
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _ = crate::effect::loader::set_assets_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets")
                .canonicalize()
                .unwrap(),
        );
        let loader = crate::effect::EffectLoader::new();
        let effect: crate::effect::format::PfxEffect =
            serde_json::from_str(include_str!("../../../../assets/effects/svg_offset.pfx"))
                .unwrap();
        let ub = crate::gpu::UniformBuffer::new(&device);
        let placeholder = crate::gpu::placeholder::PlaceholderTexture::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba16Float,
        );
        let audio = crate::gpu::audio_textures::AudioTextures::new(&device, &queue);
        let mut executor = crate::gpu::pass_executor::PassExecutor::new(
            &device,
            wgpu::TextureFormat::Rgba16Float,
            64,
            64,
            &effect.normalized_passes(),
            &loader,
            &ub,
            &placeholder,
            &audio,
            &queue,
            None,
        )
        .unwrap();
        executor.svg_offset = Some(
            SvgOffsetSim::new(&device, &queue, wgpu::TextureFormat::Rgba16Float, 64, 64).unwrap(),
        );
        executor.rebuild_all_bind_groups(&device, &ub, &placeholder, &audio);
        let mut params = crate::params::ParamStore::new();
        params.load_from_defs(&effect.inputs);
        let uniforms = crate::gpu::ShaderUniforms {
            params: params.pack_to_buffer(),
            ..Zeroable::zeroed()
        };
        for (frame, size) in [64, 96, 64].into_iter().enumerate() {
            executor.resize(&device, &queue, size, size, &ub, &placeholder, &audio);
            let mut encoder = device.create_command_encoder(&Default::default());
            executor.execute(&mut encoder, &ub, &queue, &uniforms);
            queue.submit([encoder.finish()]);
            if frame < 2 {
                executor.flip();
            }
        }
        let pixels =
            crate::gpu::test_gpu::snapshot(&device, &queue, &executor.final_targets().0.view, 64);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let validation = pollster::block_on(device.pop_error_scope());
        assert!(validation.is_none(), "{validation:?}");
        assert!(
            pixels.iter().any(|&v| v != 0),
            "native executor must draw visible contours"
        );
        // Readback proves the two transforms and atomic inradius, not just shader compilation.
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect x="2" y="2" width="12" height="14"/></svg>"#).unwrap();
        sim.reload(
            &device,
            &queue,
            file.path().to_str(),
            Default::default(),
            64,
            64,
        )
        .unwrap();
        let mask = sim.document.rasterize(64, 64).unwrap();
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 64 * 64 * 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            sim.field._field.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(64 * 16),
                    rows_per_image: Some(64),
                },
            },
            sim.field._field.size(),
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let data = readback.slice(..).get_mapped_range();
        let values: &[f32] = bytemuck::cast_slice(&data);
        let inside = jfa(&mask, true);
        let outside = jfa(&mask, false);
        let mut expected_radius = 0.5f32;
        for i in 0..64 * 64 {
            let s = if mask.ids[i] == 0 {
                outside[i]
            } else {
                inside[i]
            }
            .unwrap();
            let dx = (s % 64) as f32 - (i % 64) as f32;
            let dy = (s / 64) as f32 - (i / 64) as f32;
            let d = (dx * dx + dy * dy).sqrt();
            let expected = if mask.ids[i] == 0 {
                (d - 0.5).max(0.5)
            } else {
                expected_radius = expected_radius.max(d + 0.5);
                -d - 0.5
            };
            assert!((values[i * 4] - expected).abs() < 1e-4, "distance at {i}");
            assert_eq!(values[i * 4 + 1], 1.0);
        }
        assert!((values[3] - expected_radius).abs() < 1e-4);
        drop(data);
        readback.unmap();
        // Optional developer capture: generated from the real native GPU renderer.
        if let Ok(dir) = std::env::var("SVG_OFFSET_CAPTURE") {
            sim.reload(&device, &queue, None, Default::default(), 352, 352)
                .unwrap();
            let rt = super::super::render_target::RenderTarget::new(
                &device,
                352,
                352,
                wgpu::TextureFormat::Rgba16Float,
                1.0,
                "svg-capture",
            );
            std::fs::create_dir_all(&dir).unwrap();
            for frame in 0..30 {
                let mut params = [
                    12.0, 0.32, 1.0, 0.15, 0.0, 0.0, 1.0, 0.0, 0.35, 1.0, 0.035, 0.0, 1.0, 0.0,
                    0.0, 0.0,
                ];
                params[13] = frame as f32 / 12.0 * 0.15;
                params[14] = frame as f32 / 12.0 * 0.035;
                let mut encoder = device.create_command_encoder(&Default::default());
                sim.render(&mut encoder, &queue, &params, &rt.view);
                queue.submit([encoder.finish()]);
                let bytes = crate::gpu::test_gpu::snapshot(&device, &queue, &rt.view, 352);
                let decode = |b: &[u8]| {
                    let h = u16::from_le_bytes([b[0], b[1]]);
                    let e = (h >> 10) & 31;
                    let m = h & 1023;
                    if e == 0 {
                        f32::from(m) * 2.0f32.powi(-24)
                    } else {
                        (1.0 + f32::from(m) / 1024.0) * 2.0f32.powi(i32::from(e) - 15)
                    }
                };
                let rgb: Vec<u8> = bytes
                    .chunks_exact(8)
                    .flat_map(|p| {
                        [decode(&p[0..2]), decode(&p[2..4]), decode(&p[4..6])]
                            .map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8)
                    })
                    .collect();
                image::save_buffer(
                    format!("{dir}/{frame:03}.png"),
                    &rgb,
                    352,
                    352,
                    image::ColorType::Rgb8,
                )
                .unwrap();
            }
        }
    }
    fn document(body: &str) -> SvgDocument {
        SvgDocument::parse(
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">{body}</svg>"#
            )
            .as_bytes(),
        )
        .unwrap()
    }
    /// Integer boundary seeds, Euclidean metric, JFA+1 (same schedule as GPU).
    fn jfa(mask: &IdMask, inside: bool) -> Vec<Option<usize>> {
        let w = mask.width as i32;
        let h = mask.height as i32;
        let mut seeds = vec![None; mask.ids.len()];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if mask.ids[i] != 0
                    && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                        let (nx, ny) = (x + dx, y + dy);
                        nx < 0
                            || ny < 0
                            || nx >= w
                            || ny >= h
                            || mask.ids[(ny * w + nx) as usize] != mask.ids[i]
                    })
                {
                    seeds[i] = Some(i);
                }
            }
        }
        let mut step = mask.width.max(mask.height).next_power_of_two() / 2;
        let mut steps = Vec::new();
        while step > 0 {
            steps.push(step as i32);
            step /= 2;
        }
        steps.push(1);
        for step in steps {
            let mut next = seeds.clone();
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    let d2 = |s: usize| {
                        let dx = s as i32 % w - x;
                        let dy = s as i32 / w - y;
                        dx * dx + dy * dy
                    };
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (nx, ny) = (x + dx * step, y + dy * step);
                            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                                continue;
                            }
                            if let Some(s) = seeds[(ny * w + nx) as usize] {
                                if inside && mask.ids[s] != mask.ids[i] {
                                    continue;
                                }
                                if next[i].is_none_or(|old| d2(s) < d2(old)) {
                                    next[i] = Some(s);
                                }
                            }
                        }
                    }
                }
            }
            seeds = next;
        }
        seeds
    }

    #[test]
    fn cpu_jfa_matches_brute_force_small_grid() {
        let mask =
            document(r#"<rect x="2" y="2" width="7" height="13"/><circle cx="14" cy="9" r="4"/>"#)
                .rasterize(26, 26)
                .unwrap();
        for inside in [false, true] {
            let result = jfa(&mask, inside);
            let boundary: Vec<usize> = result
                .iter()
                .enumerate()
                .filter_map(|(i, s)| (*s == Some(i)).then_some(i))
                .collect();
            for (i, seed) in result.iter().enumerate() {
                if inside && mask.ids[i] == 0 {
                    continue;
                }
                let d2 = |s: usize| {
                    let dx = (s % 26) as i32 - (i % 26) as i32;
                    let dy = (s / 26) as i32 - (i / 26) as i32;
                    dx * dx + dy * dy
                };
                let expected = boundary
                    .iter()
                    .filter(|&&s| !inside || mask.ids[s] == mask.ids[i])
                    .map(|&s| d2(s))
                    .min()
                    .unwrap();
                assert_eq!(d2(seed.unwrap()), expected, "pixel {i}, inside={inside}");
            }
        }
    }

    #[test]
    fn svg_load_and_flatten() {
        let doc = document(
            r##"<defs><rect id="r" width="4" height="4"/></defs><g id="a" transform="translate(3 2)"><use href="#r"/><path d="M0 10 Q5 0 10 10" fill="none" stroke="red"/></g>"##,
        );
        assert_eq!(doc.shape_count(), 2);
        assert!(doc.shapes.iter().all(|s| !s.parts.is_empty()));
        assert!(
            SvgDocument::parse(DEFAULT_SVG.as_bytes())
                .unwrap()
                .shape_count()
                > 5
        );
        assert!(SvgDocument::parse(b"broken").is_err());
        assert!(doc.rasterize(0, 1).is_err());
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), DEFAULT_SVG).unwrap();
        assert!(SvgDocument::load(file.path()).is_ok());
    }
    #[test]
    fn id_mask_preserves_overlap_holes_transforms_and_strokes() {
        let doc = document(
            r##"<rect width="20" height="20" fill="#ff0000"/><g transform="translate(4 4)"><path fill="blue" fill-rule="evenodd" d="M0 0H12V12H0Z M4 4H8V8H4Z"/></g><path d="M2 2H18" stroke="lime" stroke-width="2"/>"##,
        );
        let mask = doc.rasterize(22, 22).unwrap();
        let at = |x: usize, y: usize| mask.ids[y * 22 + x];
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(2, 18), 1);
        assert_eq!(at(6, 6), 2);
        assert_eq!(at(11, 11), 1);
        assert_eq!(at(11, 3), 3);
        assert!(mask.metadata[2].geometry[3] > 0.0);
        assert_eq!(mask.metadata[1].color, [1.0, 0.0, 0.0, 1.0]);
    }
}
