use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::document::Document;
use crate::tile::Tile;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct Uniforms {
    pub cell_origin: [f32; 2],
    pub cell_span: [f32; 2],
    pub canvas_wh: [u32; 2],
    pub _pad0: [u32; 2],
}

/// Persistent wgpu state for the canvas renderer. One of these is created at
/// startup and stored in `egui_wgpu::Renderer::callback_resources`.
pub struct CanvasRenderResources {
    pub pipeline: wgpu::RenderPipeline,
    pub global_bgl: wgpu::BindGroupLayout,
    pub layer_bgl: wgpu::BindGroupLayout,

    pub uniform_buffer: wgpu::Buffer,
    pub font_sampler: wgpu::Sampler,

    // Font atlas — rebuilt when font generation changes.
    pub font: Option<FontGpu>,
    pub global_bind_group: Option<wgpu::BindGroup>,

    // Per-layer textures — rebuilt on resize or layer count change.
    pub canvas_w: u32,
    pub canvas_h: u32,
    pub layers: Vec<LayerGpu>,

    pub last_font_generation: u64,
    pub last_layout_generation: u64,
}

pub struct FontGpu {
    pub tex: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub atlas_w: u32,
    pub atlas_h: u32,
}

pub struct LayerGpu {
    pub glyph_tex: wgpu::Texture,
    pub glyph_view: wgpu::TextureView,
    pub fg_tex: wgpu::Texture,
    pub fg_view: wgpu::TextureView,
    pub bg_tex: wgpu::Texture,
    pub bg_view: wgpu::TextureView,
    pub bind_group: wgpu::BindGroup,
}

impl CanvasRenderResources {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("txpaint.canvas.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let global_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("txpaint.canvas.global_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let layer_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("txpaint.canvas.layer_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("txpaint.canvas.pipeline_layout"),
            bind_group_layouts: &[Some(&global_bgl), Some(&layer_bgl)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("txpaint.canvas.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("txpaint.canvas.uniforms"),
            contents: bytemuck::bytes_of(&Uniforms {
                cell_origin: [0.0, 0.0],
                cell_span: [1.0, 1.0],
                canvas_wh: [1, 1],
                _pad0: [0, 0],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let font_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("txpaint.canvas.font_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            global_bgl,
            layer_bgl,
            uniform_buffer,
            font_sampler,
            font: None,
            global_bind_group: None,
            canvas_w: 0,
            canvas_h: 0,
            layers: Vec::new(),
            last_font_generation: 0,
            last_layout_generation: 0,
        }
    }

    fn rebuild_font(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas_w: u32,
        atlas_h: u32,
        mask: &[u8],
    ) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("txpaint.canvas.font_tex"),
            size: wgpu::Extent3d {
                width: atlas_w,
                height: atlas_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            mask,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas_w),
                rows_per_image: Some(atlas_h),
            },
            wgpu::Extent3d {
                width: atlas_w,
                height: atlas_h,
                depth_or_array_layers: 1,
            },
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("txpaint.canvas.global_bg"),
            layout: &self.global_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.font_sampler),
                },
            ],
        });
        self.font = Some(FontGpu {
            tex,
            view,
            atlas_w,
            atlas_h,
        });
        self.global_bind_group = Some(bind_group);
    }

    fn rebuild_layers(&mut self, device: &wgpu::Device, w: u32, h: u32, layer_count: usize) {
        self.canvas_w = w;
        self.canvas_h = h;
        self.layers.clear();
        for i in 0..layer_count {
            self.layers.push(make_layer_gpu(device, &self.layer_bgl, w, h, i));
        }
    }

    pub fn ensure(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, req: &CanvasRenderRequest) {
        if self.font.is_none() || self.last_font_generation != req.font_generation {
            self.rebuild_font(device, queue, req.atlas_w, req.atlas_h, &req.font_mask);
            self.last_font_generation = req.font_generation;
        }
        if self.canvas_w != req.canvas_w
            || self.canvas_h != req.canvas_h
            || self.layers.len() != req.layers.len()
            || self.last_layout_generation != req.layout_generation
        {
            self.rebuild_layers(device, req.canvas_w, req.canvas_h, req.layers.len());
            self.last_layout_generation = req.layout_generation;
        }
    }

    fn upload_layer_full(&self, queue: &wgpu::Queue, layer_index: usize, tiles: &[Tile]) {
        let l = &self.layers[layer_index];
        let w = self.canvas_w;
        let h = self.canvas_h;

        let mut glyphs = Vec::with_capacity((w * h) as usize);
        let mut fgs = Vec::with_capacity((w * h * 4) as usize);
        let mut bgs = Vec::with_capacity((w * h * 4) as usize);
        for t in tiles {
            glyphs.push(t.glyph);
            fgs.extend_from_slice(&t.fg.0);
            bgs.extend_from_slice(&t.bg.0);
        }

        let extent = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &l.glyph_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &glyphs,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w),
                rows_per_image: Some(h),
            },
            extent,
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &l.fg_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &fgs,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            extent,
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &l.bg_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bgs,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            extent,
        );
    }

    fn upload_cells(&self, queue: &wgpu::Queue, layer_index: usize, cells: &[(u32, u32, Tile)]) {
        let l = &self.layers[layer_index];
        let one = wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        };
        for &(x, y, t) in cells {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &l.glyph_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                std::slice::from_ref(&t.glyph),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1),
                    rows_per_image: Some(1),
                },
                one,
            );
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &l.fg_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &t.fg.0,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                one,
            );
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &l.bg_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &t.bg.0,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                one,
            );
        }
    }
}

fn make_layer_gpu(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    w: u32,
    h: u32,
    index: usize,
) -> LayerGpu {
    let extent = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
    let glyph_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("txpaint.canvas.layer{index}.glyph")),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Uint,
        usage,
        view_formats: &[],
    });
    let fg_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("txpaint.canvas.layer{index}.fg")),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage,
        view_formats: &[],
    });
    let bg_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("txpaint.canvas.layer{index}.bg")),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage,
        view_formats: &[],
    });
    let glyph_view = glyph_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let fg_view = fg_tex.create_view(&wgpu::TextureViewDescriptor::default());
    let bg_view = bg_tex.create_view(&wgpu::TextureViewDescriptor::default());

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("txpaint.canvas.layer{index}.bg_group")),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&glyph_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&fg_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&bg_view),
            },
        ],
    });
    LayerGpu {
        glyph_tex,
        glyph_view,
        fg_tex,
        fg_view,
        bg_tex,
        bg_view,
        bind_group,
    }
}

/// Snapshot of all the data the renderer needs for one frame. Constructed from
/// the Document each frame and shipped to the paint callback.
pub struct CanvasRenderRequest {
    pub canvas_w: u32,
    pub canvas_h: u32,
    pub atlas_w: u32,
    pub atlas_h: u32,
    pub font_mask: Arc<Vec<u8>>,
    pub font_generation: u64,
    pub layout_generation: u64,
    pub cell_origin: [f32; 2],
    pub cell_span: [f32; 2],
    /// Per-layer data. A `full_tiles` Some value means do a full re-upload; dirty
    /// cells are always applied on top (typically you'd use one or the other).
    pub layers: Vec<LayerRenderRequest>,
}

pub struct LayerRenderRequest {
    pub visible: bool,
    pub full_tiles: Option<Arc<Vec<Tile>>>,
    pub dirty_cells: Vec<(u32, u32, Tile)>,
}

impl CanvasRenderRequest {
    /// Build a render request from the current document, taking ownership of any
    /// per-layer dirty cells (resets them on the document).
    ///
    /// `cell_origin` is the canvas cell at the top-left of the viewport (can be
    /// fractional for sub-cell pan), `cell_span` is the number of canvas cells
    /// covered by the viewport in x/y.
    pub fn from_document(
        document: &mut Document,
        cell_origin: [f32; 2],
        cell_span: [f32; 2],
    ) -> Self {
        let mut layers = Vec::with_capacity(document.layers.len());
        for layer in document.layers.iter_mut() {
            let full_tiles = if layer.full_upload {
                layer.full_upload = false;
                layer.dirty_cells.clear();
                Some(Arc::new(layer.tiles.clone()))
            } else {
                None
            };
            let mut dirty_cells = Vec::with_capacity(layer.dirty_cells.len());
            if full_tiles.is_none() {
                for (x, y) in layer.dirty_cells.drain() {
                    let idx = (y * document.width + x) as usize;
                    dirty_cells.push((x, y, layer.tiles[idx]));
                }
            }
            layers.push(LayerRenderRequest {
                visible: layer.visible,
                full_tiles,
                dirty_cells,
            });
        }
        Self {
            canvas_w: document.width,
            canvas_h: document.height,
            atlas_w: document.font.atlas_w(),
            atlas_h: document.font.atlas_h(),
            font_mask: Arc::new(document.font.mask.clone()),
            font_generation: document.resources_generation,
            layout_generation: document.resources_generation,
            cell_origin,
            cell_span,
            layers,
        }
    }
}

/// Paint callback that renders the canvas inside an egui paint region.
pub struct CanvasCallback {
    pub request: CanvasRenderRequest,
}

impl egui_wgpu::CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(res) = callback_resources.get_mut::<CanvasRenderResources>() else {
            return Vec::new();
        };
        res.ensure(device, queue, &self.request);

        let uniforms = Uniforms {
            cell_origin: self.request.cell_origin,
            cell_span: self.request.cell_span,
            canvas_wh: [self.request.canvas_w, self.request.canvas_h],
            _pad0: [0, 0],
        };
        queue.write_buffer(&res.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        for (i, layer_req) in self.request.layers.iter().enumerate() {
            if let Some(tiles) = &layer_req.full_tiles {
                res.upload_layer_full(queue, i, tiles.as_slice());
            }
            if !layer_req.dirty_cells.is_empty() {
                res.upload_cells(queue, i, &layer_req.dirty_cells);
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(res) = callback_resources.get::<CanvasRenderResources>() else {
            return;
        };
        let Some(global_bg) = res.global_bind_group.as_ref() else {
            return;
        };
        if res.layers.is_empty() {
            return;
        }
        render_pass.set_pipeline(&res.pipeline);
        render_pass.set_bind_group(0, global_bg, &[]);
        for (i, layer_req) in self.request.layers.iter().enumerate() {
            if !layer_req.visible {
                continue;
            }
            render_pass.set_bind_group(1, &res.layers[i].bind_group, &[]);
            render_pass.draw(0..6, 0..1);
        }
    }
}
