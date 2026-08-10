use std::borrow::Cow;

use wgpu::{
    util::DeviceExt, Color, CommandEncoderDescriptor, CurrentSurfaceTexture, Device,
    DeviceDescriptor, Features, FragmentState, InstanceDescriptor, Limits, LoadOp, MemoryHints,
    Operations, PowerPreference, Queue, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, RequestAdapterOptions, ShaderModuleDescriptor,
    ShaderSource, StoreOp, Surface, SurfaceConfiguration, TextureFormat, TextureViewDescriptor,
    VertexState,
};
use winit::{dpi::PhysicalSize, event_loop::EventLoopProxy, window::Window};

#[cfg(target_arch = "wasm32")]
pub type Rc<T> = std::rc::Rc<T>;

#[cfg(not(target_arch = "wasm32"))]
pub type Rc<T> = std::sync::Arc<T>;

pub async fn create_graphics(window: Rc<Window>, proxy: EventLoopProxy<Graphics>) {
    // Not `Instance::default()`. On the web, wgpu decides whether to use WebGPU when the
    // instance is created, and a plain `Instance::new` only checks that `navigator.gpu`
    // exists. Browsers can expose that object yet still fail to produce any WebGPU
    // adapter, in which case the instance is locked to WebGPU and never falls back to
    // WebGL. This helper probes for a real adapter first and drops BROWSER_WEBGPU if
    // there isn't one, so the `webgl` feature can actually take over.
    let instance = wgpu::util::new_instance_with_webgpu_detection(
        InstanceDescriptor::new_without_display_handle(),
    )
    .await;
    let surface = instance.create_surface(Rc::clone(&window)).unwrap();
    let adapter = instance
        .request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::default(), // Power preference for the device
            force_fallback_adapter: false, // Indicates that only a fallback ("software") adapter can be used
            compatible_surface: Some(&surface), // Guarantee that the adapter can render to this surface
            apply_limit_buckets: false, // Rounds limits into coarse buckets to reduce fingerprinting. Only useful when exposing wgpu to untrusted content.
        })
        .await
        .expect("Could not get an adapter (GPU).");

    let (device, queue) = adapter
        .request_device(&DeviceDescriptor {
            label: None,
            required_features: Features::empty(), // Specifies the required features by the device request. Fails if the adapter can't provide them.
            required_limits: Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            memory_hints: MemoryHints::Performance,
            trace: Default::default(),
            experimental_features: Default::default(),
        })
        .await
        .expect("Failed to get device");

    // Get physical pixel dimensions inside the window
    let size = window.inner_size();
    // Make the dimensions at least size 1, otherwise wgpu would panic
    let width = size.width.max(1);
    let height = size.height.max(1);
    let mut surface_config = surface.get_default_config(&adapter, width, height).unwrap();

    // `get_default_config` picks the first present mode the surface reports, which
    // varies by platform and driver (often Mailbox, which renders uncapped). Pin Fifo
    // so the render loop is vsync-limited everywhere. Swap to Mailbox or Immediate for
    // uncapped frames.
    surface_config.present_mode = wgpu::PresentMode::Fifo;

    surface.configure(&device, &surface_config);

    let render_pipeline = create_pipeline(&device, surface_config.format);

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Vertex Buffer"),
        contents: &[0; 100 * size_of::<Vertex>()],
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Index Buffer"),
        contents: &[0; 100 * size_of::<u32>()],
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
    });

    let gfx = Graphics {
        window: window.clone(),
        surface,
        surface_config,
        device,
        queue,
        render_pipeline,
        vertex_buffer,
        index_buffer,
        num_indices: 1,
        size,
    };

    let _ = proxy.send_event(gfx);
}

fn create_pipeline(device: &Device, swap_chain_format: TextureFormat) -> RenderPipeline {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
    });

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[Vertex::desc()],
            compilation_options: Default::default(),
        },
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(swap_chain_format.into())],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[derive(Debug)]
pub struct Graphics {
    window: Rc<Window>,
    surface: Surface<'static>,
    surface_config: SurfaceConfiguration,
    device: Device,
    queue: Queue,
    render_pipeline: RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    size: winit::dpi::PhysicalSize<u32>,
}

impl Graphics {
    pub fn size(&self) -> (u32, u32) {
        (self.surface_config.width, self.surface_config.height)
    }
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        self.surface_config.width = new_size.width.max(1);
        self.surface_config.height = new_size.height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        self.size = new_size;
    }

    pub fn draw(&mut self) {
        // `get_current_texture` reports why acquisition didn't yield a usable frame
        // rather than collapsing it into one error, so each case is handled on its own.
        let frame = match self.surface.get_current_texture() {
            // Suboptimal still presents correctly, it just no longer matches the surface.
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            // The surface configuration is stale. Reconfigure at the window's *current*
            // size — reusing the stored config would keep the same stale dimensions and
            // the surface would report Outdated forever, so no frame is ever acquired.
            CurrentSurfaceTexture::Outdated | CurrentSurfaceTexture::Lost => {
                let size = self.window.inner_size();
                self.resize(size);
                return;
            }
            // Transient, skip this frame and try again on the next one.
            CurrentSurfaceTexture::Timeout
            | CurrentSurfaceTexture::Occluded
            | CurrentSurfaceTexture::Validation => return,
        };

        let view = frame.texture.create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });

        {
            let mut r_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::BLUE),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            r_pass.set_pipeline(&self.render_pipeline);
            r_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            r_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            r_pass.draw_indexed(0..self.num_indices, 0, 0..1);
        } // `r_pass` dropped here

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
    }

    pub fn push_vertices(&mut self, vertices: Vec<Vertex>, indices: &[u32]) {
        if self.vertex_buffer.size() as usize != size_of::<Vertex>() * vertices.len() {
            self.vertex_buffer.destroy();
            self.vertex_buffer =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Vertex Buffer"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    });
        } else {
            self.queue
                .write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        }
        if self.index_buffer.size() as usize != size_of::<u32>() * indices.len() {
            self.index_buffer.destroy();
            self.index_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Index Buffer"),
                    contents: bytemuck::cast_slice(indices),
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                });
            self.num_indices = indices.len() as u32;
        } else {
            self.queue
                .write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(indices));
        }
    }
}

// lib.rs
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
    pub color: f32,
}

impl Vertex {
    fn desc() -> Option<wgpu::VertexBufferLayout<'static>> {
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        })
    }
}
