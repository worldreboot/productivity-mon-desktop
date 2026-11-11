//! Draws baked models into see-through windows. One device for every window; a model is one vertex buffer, one
//! index buffer and a draw for each of its materials. The lighting is a sun and a sky, as the game's models have.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use winit::window::Window;

use crate::anim::Pose;
use crate::mon::{Mon, STRIDE};

const SHADER: &str = include_str!("mon.wgsl");
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Samples a pixel: the edge of a mon is all the desktop sees of how it is drawn.
const SAMPLES: u32 = 4;

fn transparent_alpha(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    for mode in [wgpu::CompositeAlphaMode::PreMultiplied,wgpu::CompositeAlphaMode::PostMultiplied] {
        if modes.contains(&mode) {return mode;}
    }
    panic!("this surface does not support transparency: {modes:?}");
}
/// How far the camera looks down on a mon, in radians.
pub const PITCH: f32 = 0.17;
/// How far down its window a mon's feet are, as a share of the window's height.
pub const FEET: f32 = 0.93;

pub struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    module: wgpu::ShaderModule,
    frame_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    /// By the surface's format, two sides or one, blended or not, writing depth or not.
    pipelines: RefCell<HashMap<(wgpu::TextureFormat, bool, bool, bool), Rc<wgpu::RenderPipeline>>>,
    alpha_pipelines: RefCell<HashMap<wgpu::TextureFormat,wgpu::RenderPipeline>>,
    white: wgpu::TextureView,
    /// Time spent getting a window's next picture, drawing into it, and showing it: what a frame costs, by part.
    pub spent: std::cell::Cell<[std::time::Duration; 3]>,
}

/// A window's own end of the drawing.
pub struct Target {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    colour: wgpu::TextureView,
    depth: wgpu::TextureView,
    alpha_layer: Option<wgpu::TextureView>,
}

struct MaterialGpu {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
    /// The texture's offset as it was last sent.
    offset: [f32; 2],
}

/// One mon's model on the GPU, with the buffers that say how this one stands.
pub struct Model {
    pub mon: Rc<Mon>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    materials: Vec<MaterialGpu>,
    frame: wgpu::Buffer,
    bones: wgpu::Buffer,
    group: wgpu::BindGroup,
    /// Half of what the window shows, up and across, in the model's units, and how high its middle is.
    half: f32,
    middle: f32,
}

fn instance() -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    #[cfg(target_os="windows")]
    { descriptor.backends = wgpu::Backends::DX12;
    // A swapchain on a DirectComposition visual: the only kind on Windows whose alpha reaches the desktop.
    descriptor.backend_options.dx12.presentation_system = wgpu::wgt::Dx12SwapchainKind::DxgiFromVisual; }
    #[cfg(target_os="macos")]
    { descriptor.backends = wgpu::Backends::METAL; }
    wgpu::Instance::new(descriptor)
}

/// One model in a window's picture: how it is posed, where it is put, and how far it is turned to white.
pub struct Item<'a> {
    pub model: &'a mut Model,
    pub pose: &'a Pose,
    pub place: Mat4,
    pub flash: f32,
    /// How much of it is there, from none to all. Only for what is see-through anyway (a plate).
    pub opacity: f32,
}

impl Gpu {
    pub fn overlay_layer(&self,width:u32,height:u32,format:wgpu::TextureFormat)->wgpu::TextureView {
        self.device.create_texture(&wgpu::TextureDescriptor {label:Some("premultiplied overlay"),size:wgpu::Extent3d {width:width.max(1),height:height.max(1),depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format,usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,view_formats:&[]}).create_view(&Default::default())
    }

    /// Metal advertises straight-alpha surfaces; all scene/UI blending stays premultiplied until this final pass.
    pub fn resolve_overlay(&self,encoder:&mut wgpu::CommandEncoder,source:&wgpu::TextureView,target:&wgpu::TextureView,format:wgpu::TextureFormat) {
        let mut pipelines=self.alpha_pipelines.borrow_mut();
        let pipeline=pipelines.entry(format).or_insert_with(|| {
            let shader=self.device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("overlay straight alpha"),source:wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into())});
            self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {label:Some("overlay straight alpha"),layout:None,vertex:wgpu::VertexState {module:&shader,entry_point:Some("vs"),buffers:&[],compilation_options:Default::default()},fragment:Some(wgpu::FragmentState {module:&shader,entry_point:Some("fs"),targets:&[Some(wgpu::ColorTargetState {format,blend:None,write_mask:wgpu::ColorWrites::ALL})],compilation_options:Default::default()}),primitive:Default::default(),depth_stencil:None,multisample:Default::default(),multiview_mask:None,cache:None})
        });
        let group=self.device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("overlay source"),layout:&pipeline.get_bind_group_layout(0),entries:&[wgpu::BindGroupEntry {binding:0,resource:wgpu::BindingResource::TextureView(source)}]});
        let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {label:Some("present overlay"),color_attachments:&[Some(wgpu::RenderPassColorAttachment {view:target,depth_slice:None,resolve_target:None,ops:wgpu::Operations {load:wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),store:wgpu::StoreOp::Store}})],..Default::default()});
        pass.set_pipeline(pipeline);pass.set_bind_group(0,&group,&[]);pass.draw(0..3,0..1);
    }

    /// Opens the GPU for the first window, and that window's target with it.
    pub fn open(window: Arc<Window>) -> (Gpu, Target) {
        let instance = instance();
        let surface = instance.create_surface(window.clone()).expect("a surface on the window");
        let gpu = Gpu::with(instance, Some(&surface));
        let target = gpu.target_on(window, surface);
        (gpu, target)
    }

    /// Opens the GPU with no window, to draw pictures into files (`snap`).
    pub fn headless() -> Gpu {
        Gpu::with(instance(), None)
    }

    fn with(instance: wgpu::Instance, surface: Option<&wgpu::Surface>) -> Gpu {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            // A laptop's small GPU is plenty for a mon, and waking the big one costs battery.
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: surface,
            ..Default::default()
        }))
        .expect("a graphics adapter for this platform");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("desk"),
            // A few small models: ask the allocator for small blocks, not a game's worth up front.
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .expect("a device");
        device.on_uncaptured_error(Arc::new(|error: wgpu::Error| eprintln!("[desk] {error}")));

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mon"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let buffer = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[
                buffer(0, wgpu::ShaderStages::VERTEX_FRAGMENT, wgpu::BufferBindingType::Uniform),
                buffer(1, wgpu::ShaderStages::VERTEX, wgpu::BufferBindingType::Storage { read_only: true }),
            ],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                buffer(0, wgpu::ShaderStages::VERTEX_FRAGMENT, wgpu::BufferBindingType::Uniform),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mon"),
            bind_group_layouts: &[Some(&frame_layout), Some(&material_layout)],
            immediate_size: 0,
        });
        let white = texture(&device, &queue, &[255, 255, 255, 255], 1, 1);
        Gpu { instance, adapter, device, queue, module, frame_layout, material_layout, layout, pipelines: RefCell::new(HashMap::new()), alpha_pipelines: RefCell::new(HashMap::new()), white, spent: Default::default() }
    }

    /// A surface on an ordinary window that is not see-through: the panel's.
    pub fn plain(&self, window: Arc<Window>) -> (wgpu::Surface<'static>, wgpu::SurfaceConfiguration) {
        self.window_surface(window, false)
    }

    /// An egui overlay whose premultiplied alpha reaches the Windows desktop.
    pub fn overlay(&self, window: Arc<Window>) -> (wgpu::Surface<'static>, wgpu::SurfaceConfiguration) {
        self.window_surface(window, true)
    }

    fn window_surface(&self, window: Arc<Window>, transparent: bool) -> (wgpu::Surface<'static>, wgpu::SurfaceConfiguration) {
        let size = window.inner_size();
        let surface = self.instance.create_surface(window).expect("a surface on the window");
        let mut config = surface.get_default_config(&self.adapter, size.width.max(1), size.height.max(1)).expect("a surface config");
        // egui writes sRGB numbers itself, as the mons' shader does.
        config.format = config.format.remove_srgb_suffix();
        config.alpha_mode = if transparent { transparent_alpha(&surface.get_capabilities(&self.adapter).alpha_modes) } else { wgpu::CompositeAlphaMode::Opaque };
        assert!(surface.get_capabilities(&self.adapter).alpha_modes.contains(&config.alpha_mode), "unsupported window alpha mode");
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&self.device, &config);
        (surface, config)
    }

    /// Another window's target.
    pub fn target(&self, window: Arc<Window>) -> Target {
        let surface = self.instance.create_surface(window.clone()).expect("a surface on the window");
        self.target_on(window, surface)
    }

    fn target_on(&self, window: Arc<Window>, surface: wgpu::Surface<'static>) -> Target {
        let size = window.inner_size();
        let caps = surface.get_capabilities(&self.adapter);
        let mut config = surface.get_default_config(&self.adapter, size.width.max(1), size.height.max(1)).expect("a surface config");
        // The shader writes sRGB numbers itself; the desktop blends them as they are.
        config.format = config.format.remove_srgb_suffix();
        config.alpha_mode = transparent_alpha(&caps.alpha_modes);
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&self.device, &config);
        let colour = self.attachment(config.width, config.height, config.format, SAMPLES);
        let depth = self.attachment(config.width, config.height, DEPTH, SAMPLES);
        let alpha_layer=(config.alpha_mode==wgpu::CompositeAlphaMode::PostMultiplied).then(||self.overlay_layer(config.width,config.height,config.format));
        Target { window, surface, config, colour, depth, alpha_layer }
    }

    fn attachment(&self, width: u32, height: u32, format: wgpu::TextureFormat, samples: u32) -> wgpu::TextureView {
        self.device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("attachment"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    fn pipeline(&self, format: wgpu::TextureFormat, two_sided: bool, blend: bool, depth_write: bool) -> Rc<wgpu::RenderPipeline> {
        let key = (format, two_sided, blend, depth_write);
        if let Some(found) = self.pipelines.borrow().get(&key) {
            return found.clone();
        }
        const CORNER: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Uint16x4, 4 => Float32x4];
        let made = Rc::new(self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mon"),
            layout: Some(&self.layout),
            vertex: wgpu::VertexState {
                module: &self.module,
                entry_point: Some("vs"),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: STRIDE as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &CORNER })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(if blend { wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING } else { wgpu::BlendState::REPLACE }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { cull_mode: if two_sided { None } else { Some(wgpu::Face::Back) }, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(depth_write),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview_mask: None,
            cache: None,
        }));
        self.pipelines.borrow_mut().insert(key, made.clone());
        made
    }

    /// Puts a baked model on the GPU.
    pub fn model(&self, mon: Rc<Mon>) -> Model {
        let device = &self.device;
        let buffer = |label, size: usize, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage, mapped_at_creation: false });
        let vertices = buffer("vertices", mon.vertex_bytes().len(), wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST);
        self.queue.write_buffer(&vertices, 0, mon.vertex_bytes());
        let indices = buffer("indices", mon.index_bytes().len(), wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST);
        self.queue.write_buffer(&indices, 0, mon.index_bytes());
        let frame = buffer("frame", FRAME_FLOATS * 4, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
        let bones = buffer("bones", mon.header.joints.len().max(1) * 64, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &self.frame_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: bones.as_entire_binding() }],
        });

        let mut pictures: HashMap<&str, wgpu::TextureView> = HashMap::new();
        let materials = mon
            .header
            .materials
            .iter()
            .map(|material| {
                let view = match &material.texture {
                    // A picture the app made itself.
                    Some(file) if file == "@pixels" => match &mon.pixels {
                        Some((pixels, width, height)) => texture(device, &self.queue, pixels, *width, *height),
                        None => self.white.clone(),
                    },
                    Some(file) => pictures
                        .entry(file.as_str())
                        .or_insert_with(|| match picture(&mon.folder.join(file)) {
                            Ok((pixels, width, height)) => texture(device, &self.queue, &pixels, width, height),
                            Err(error) => {
                                eprintln!("[desk] {file}: {error}");
                                self.white.clone()
                            }
                        })
                        .clone(),
                    None => self.white.clone(),
                };
                let wrap = |code| [wgpu::AddressMode::Repeat, wgpu::AddressMode::ClampToEdge, wgpu::AddressMode::MirrorRepeat][code as usize % 3];
                let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("map"),
                    address_mode_u: wrap(material.wrap[0]),
                    address_mode_v: wrap(material.wrap[1]),
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    mipmap_filter: wgpu::MipmapFilterMode::Linear,
                    anisotropy_clamp: 8,
                    ..Default::default()
                });
                let buffer = buffer("material", MATERIAL_FLOATS * 4, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("material"),
                    layout: &self.material_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
                    ],
                });
                // Not a number: the first frame sends it whatever it is.
                MaterialGpu { buffer, group, offset: [f32::NAN; 2] }
            })
            .collect();

        // What the window must show: the mon turned any way round, with room over its head for what its clips do.
        let bounds = &mon.header.bounds;
        let reach = bounds.min[0].abs().max(bounds.max[0].abs()).max(bounds.min[2].abs()).max(bounds.max[2].abs());
        let half = (reach * 1.2).max(bounds.max[1] * 0.62);
        drop(pictures);
        Model { mon, vertices, indices, materials, frame, bones, group, half, middle: half * (2.0 * FEET - 1.0) }
    }

    /// Draws models into a window, seen by the camera of the mon whose window it is: `Model::camera`'s two, and how
    /// much more than a square the window shows above it.
    pub fn draw(&self, target: &Target, camera: (f32, f32, f32, f32), scene: &mut [Item]) {
        let began = std::time::Instant::now();
        let texture = match target.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            // Covered, minimised or mid-change: nothing to show this time.
            _ => return,
        };
        let got = std::time::Instant::now();
        let resolve = texture.texture.create_view(&Default::default());
        self.pass(&target.colour, target.alpha_layer.as_ref().unwrap_or(&resolve), &target.depth, target.config.format, camera, scene);
        if let Some(layer)=&target.alpha_layer {
            let mut encoder=self.device.create_command_encoder(&Default::default());
            self.resolve_overlay(&mut encoder,layer,&resolve,target.config.format);
            self.queue.submit([encoder.finish()]);
        }
        let drew = std::time::Instant::now();
        target.window.pre_present_notify();
        self.queue.present(texture);
        let mut spent = self.spent.get();
        spent[0] += got - began;
        spent[1] += drew - got;
        spent[2] += drew.elapsed();
        self.spent.set(spent);
    }

    /// Draws models as `draw` does into a picture `size` pixels square, and gives its pixels: RGBA, top row
    /// first, colour multiplied by alpha, as the desktop is given them.
    pub fn snap(&self, camera: (f32, f32, f32, f32), scene: &mut [Item], width: u32, height: u32) -> Vec<u8> {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let picture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("snap"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let colour = self.attachment(width, height, format, SAMPLES);
        let depth = self.attachment(width, height, DEPTH, SAMPLES);
        self.pass(&colour, &picture.create_view(&Default::default()), &depth, format, camera, scene);
        // Rows are padded to 256 bytes on their way out of the GPU.
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snap"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &picture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) } },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |result| result.expect("the picture read back"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU to finish");
        let mapped = buffer.slice(..).get_mapped_range().expect("the picture mapped");
        mapped.chunks_exact(row as usize).flat_map(|line| line[..(width * 4) as usize].to_vec()).collect()
    }

    fn pass(&self, colour: &wgpu::TextureView, resolve: &wgpu::TextureView, depth: &wgpu::TextureView, format: wgpu::TextureFormat, (half, middle, extra, wide): (f32, f32, f32, f32), scene: &mut [Item]) {
        // `wide` is how many times wider than the mon's square the picture is: room at its sides for a plate wider
        // than a small mon.
        // `extra` is how much more than a square the picture shows above the mon, in its units: room for its plate.
        let far = half * 8.0;
        let up = half + extra / 2.0;
        let centre = Vec3::new(0.0, middle + extra / 2.0, 0.0);
        let eye = centre + Vec3::new(0.0, PITCH.sin(), PITCH.cos()) * far * 0.5;
        let view_proj = Mat4::orthographic_rh(-half * wide, half * wide, -up, up, 0.0, far) * Mat4::look_at_rh(eye, centre, Vec3::Y);
        for item in scene.iter_mut() {
            let mut frame = [0f32; FRAME_FLOATS];
            frame[..16].copy_from_slice(&view_proj.to_cols_array());
            frame[16..32].copy_from_slice(&item.place.to_cols_array());
            frame[32..35].copy_from_slice(&Vec3::new(-0.35, 0.75, 0.56).normalize().to_array());
            frame[35] = item.flash;
            frame[36] = item.opacity;
            self.queue.write_buffer(&item.model.frame, 0, bytemuck::cast_slice(&frame));
            self.queue.write_buffer(&item.model.bones, 0, bytemuck::cast_slice(&item.pose.palette));
            let mon = item.model.mon.clone();
            for (source, material) in mon.header.materials.iter().zip(&mut item.model.materials) {
                // Eyes and mouths are other places on their textures, which a node's place says.
                let moved = if source.uv_node >= 0 { item.pose.locals[source.uv_node as usize].t } else { Vec3::ZERO };
                let offset = [source.offset[0] - moved.x, source.offset[1] + moved.y];
                if offset != material.offset {
                    material.offset = offset;
                    let c = source.colour;
                    let floats: [f32; MATERIAL_FLOATS] = [c[0], c[1], c[2], 1.0, source.repeat[0], source.repeat[1], offset[0], offset[1], source.cut, if source.blend { 1.0 } else { 0.0 }, if source.unlit { 1.0 } else { 0.0 }, 0.0];
                    self.queue.write_buffer(&material.buffer, 0, bytemuck::cast_slice(&floats));
                }
            }
        }

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mon"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: colour,
                    depth_slice: None,
                    resolve_target: Some(resolve),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            for item in scene.iter() {
                let (model, pose, mon) = (&*item.model, item.pose, &item.model.mon);
                pass.set_bind_group(0, &model.group, &[]);
                pass.set_vertex_buffer(0, model.vertices.slice(..));
                pass.set_index_buffer(model.indices.slice(..), wgpu::IndexFormat::Uint32);
                // What is solid first, then what is seen through, each in the order the model has them.
                for blended in [false, true] {
                    for group in &mon.header.groups {
                        let material = &mon.header.materials[group.material];
                        // HOME shows a part for a node at a negative x in Unity, which is a positive one here.
                        let shown = if group.show_node >= 0 { pose.locals[group.show_node as usize].t.x > 0.0 } else { group.shown };
                        if material.blend != blended || !shown || group.count == 0 {
                            continue;
                        }
                        pass.set_pipeline(&self.pipeline(format, material.two_sided, material.blend, material.depth_write));
                        pass.set_bind_group(1, &model.materials[group.material].group, &[]);
                        pass.draw_indexed(group.first..group.first + group.count, 0, 0..1);
                    }
                }
            }
        }
        self.queue.submit([encoder.finish()]);
    }
}

impl Target {
    pub fn resize(&mut self,gpu:&Gpu,width:u32,height:u32) {
        if width==0||height==0{return;}
        self.config.width=width;self.config.height=height;
        self.surface.configure(&gpu.device,&self.config);
        self.colour=gpu.attachment(width,height,self.config.format,SAMPLES);
        self.depth=gpu.attachment(width,height,DEPTH,SAMPLES);
        self.alpha_layer=(self.config.alpha_mode==wgpu::CompositeAlphaMode::PostMultiplied).then(||gpu.overlay_layer(width,height,self.config.format));
    }
}

impl Model {
    /// What its window's camera shows: half of it up and across, in the model's units, and how high its middle is.
    pub fn camera(&self) -> (f32, f32) {
        (self.half, self.middle)
    }

    /// How many of the window's pixels one of the model's units is drawn.
    pub fn pixels_a_unit(&self, height: u32) -> f32 {
        height as f32 / (2.0 * self.half)
    }
}

const FRAME_FLOATS: usize = 40;
const MATERIAL_FLOATS: usize = 12;

/// A picture as a texture with all its smaller copies.
fn texture(device: &wgpu::Device, queue: &wgpu::Queue, pixels: &[u8], width: u32, height: u32) -> wgpu::TextureView {
    let levels = 32 - width.max(height).leading_zeros();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("picture"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let (mut level, mut w, mut h) = (pixels.to_vec(), width, height);
    for mip in 0..levels {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: mip, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &level,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        // Half the size, each pixel the mean of the four (or two) over it.
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |px: u32, py: u32| level[((py.min(h - 1) * w + px.min(w - 1)) * 4 + c) as usize] as u32;
                    next[((y * nw + x) * 4 + c) as usize] = ((at(x * 2, y * 2) + at(x * 2 + 1, y * 2) + at(x * 2, y * 2 + 1) + at(x * 2 + 1, y * 2 + 1) + 2) / 4) as u8;
                }
            }
        }
        (level, w, h) = (next, nw, nh);
    }
    texture.create_view(&Default::default())
}

/// Reads a PNG as RGBA bytes, top row first.
pub fn picture(path: &std::path::Path) -> Result<(Vec<u8>, u32, u32), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut bytes = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut bytes).map_err(|e| e.to_string())?;
    bytes.truncate(info.buffer_size());
    let pixels = match info.color_type {
        png::ColorType::Rgba => bytes,
        png::ColorType::Rgb => bytes.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => bytes.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => bytes.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Indexed => return Err("a palette the decoder did not expand".into()),
    };
    Ok((pixels, info.width, info.height))
}

#[cfg(test)]
mod alpha_tests {
    use super::*;
    #[test]
    #[ignore="GPU readback check; run explicitly on a graphics-capable machine"]
    fn straight_alpha_overlay_preserves_transparency_and_color() {
        let gpu=Gpu::headless();
        let make=|usage|gpu.device.create_texture(&wgpu::TextureDescriptor {label:Some("alpha probe"),size:wgpu::Extent3d {width:2,height:1,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba8Unorm,usage,view_formats:&[]});
        let source=make(wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST);
        let target=make(wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC);
        gpu.queue.write_texture(wgpu::TexelCopyTextureInfo {texture:&source,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},&[32,16,8,64,9,7,3,0],wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(8),rows_per_image:Some(1)},wgpu::Extent3d {width:2,height:1,depth_or_array_layers:1});
        let buffer=gpu.device.create_buffer(&wgpu::BufferDescriptor {label:Some("alpha readback"),size:256,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
        let mut encoder=gpu.device.create_command_encoder(&Default::default());
        gpu.resolve_overlay(&mut encoder,&source.create_view(&Default::default()),&target.create_view(&Default::default()),wgpu::TextureFormat::Rgba8Unorm);
        encoder.copy_texture_to_buffer(wgpu::TexelCopyTextureInfo {texture:&target,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},wgpu::TexelCopyBufferInfo {buffer:&buffer,layout:wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(256),rows_per_image:Some(1)}},wgpu::Extent3d {width:2,height:1,depth_or_array_layers:1});
        gpu.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read,|result|result.unwrap());
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let pixels=buffer.slice(..).get_mapped_range().unwrap();
        for (actual,expected) in pixels[..3].iter().zip([128u8,64,32]) {assert!(actual.abs_diff(expected)<=1,"unorm quantization: {actual} vs {expected}");}
        assert_eq!(&pixels[3..8],&[64,0,0,0,0]);
    }
}
