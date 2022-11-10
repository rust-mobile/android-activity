use std::borrow::Cow;

use log::trace;

use wgpu::TextureFormat;
use wgpu::{Adapter, Device, Instance, PipelineLayout, Queue, RenderPipeline, ShaderModule};

use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopWindowTarget},
};

#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

struct RenderState {
    device: Device,
    queue: Queue,
    _shader: ShaderModule,
    target_format: TextureFormat,
    _pipeline_layout: PipelineLayout,
    render_pipeline: RenderPipeline,
}

struct SurfaceState {
    window: winit::window::Window,
    surface: wgpu::Surface,
}

struct App {
    instance: Instance,
    adapter: Option<Adapter>,
    surface_state: Option<SurfaceState>,
    render_state: Option<RenderState>,
}

impl App {
    fn new(instance: Instance) -> Self {
        Self {
            instance,
            adapter: None,
            surface_state: None,
            render_state: None,
        }
    }
}

impl App {
    fn create_surface<T>(&mut self, event_loop: &EventLoopWindowTarget<T>) {
        let window = winit::window::Window::new(&event_loop).unwrap();
        trace!("WGPU: creating surface for native window");
        let surface = unsafe { self.instance.create_surface(&window) };
        self.surface_state = Some(SurfaceState { window, surface });
    }

    async fn init_render_state(adapter: &Adapter, target_format: TextureFormat) -> RenderState {
        trace!("Initializing render state");

        trace!("WGPU: requesting device");
        // Create the logical device and command queue
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty(),
                    // Make sure we use the texture resolution limits from the adapter, so we can support images the size of the swapchain.
                    limits: wgpu::Limits::downlevel_webgl2_defaults()
                        .using_resolution(adapter.limits()),
                },
                None,
            )
            .await
            .expect("Failed to create device");

        trace!("WGPU: loading shader");
        // Load the shaders from disk
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        trace!("WGPU: creating pipeline layout");
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        trace!("WGPU: creating render pipeline");
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(target_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        RenderState {
            device,
            queue,
            _shader: shader,
            target_format,
            _pipeline_layout: pipeline_layout,
            render_pipeline,
        }
    }

    // We want to defer the initialization of our render state until
    // we have a surface so we can take its format into account.
    //
    // After we've initialized our render state once though we
    // expect all future surfaces will have the same format and we
    // so this stat will remain valid.
    async fn ensure_render_state_for_surface(&mut self) {
        if let Some(surface_state) = &self.surface_state {
            if self.adapter.is_none() {
                trace!("WGPU: requesting a suitable adapter (compatible with our surface)");
                let adapter = self
                    .instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::default(),
                        force_fallback_adapter: false,
                        // Request an adapter which can render to our surface
                        compatible_surface: Some(&surface_state.surface),
                    })
                    .await
                    .expect("Failed to find an appropriate adapter");

                self.adapter = Some(adapter);
            }
            let adapter = self.adapter.as_ref().unwrap();

            if self.render_state.is_none() {
                trace!("WGPU: finding supported swapchain format");
                let swapchain_format = surface_state.surface.get_supported_formats(&adapter)[0];

                let rs = Self::init_render_state(adapter, swapchain_format).await;
                self.render_state = Some(rs);
            }
        }
    }

    fn configure_surface_swapchain(&mut self) {
        if let (Some(render_state), Some(surface_state)) = (&self.render_state, &self.surface_state)
        {
            let swapchain_format = render_state.target_format;
            let size = surface_state.window.inner_size();

            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: swapchain_format,
                width: size.width,
                height: size.height,
                present_mode: wgpu::PresentMode::Mailbox,
                //present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Inherit,
            };

            trace!("WGPU: Configuring surface swapchain: format = {swapchain_format:?}, size = {size:?}");
            surface_state
                .surface
                .configure(&render_state.device, &config);
        }
    }

    fn queue_redraw(&self) {
        if let Some(surface_state) = &self.surface_state {
            trace!("Making Redraw Request");
            surface_state.window.request_redraw();
        }
    }

    fn resume<T>(&mut self, event_loop: &EventLoopWindowTarget<T>) {
        trace!("Resumed, creating render state...");
        self.create_surface(event_loop);
        pollster::block_on(self.ensure_render_state_for_surface());
        self.configure_surface_swapchain();
        self.queue_redraw();
    }
}

fn run(event_loop: EventLoop<()>) {
    trace!("Running mainloop...");

    // doesn't need to be re-considered later
    let instance = wgpu::Instance::new(wgpu::Backends::all());
    //let instance = wgpu::Instance::new(wgpu::Backends::VULKAN);
    //let instance = wgpu::Instance::new(wgpu::Backends::GL);

    let mut app = App::new(instance);
    event_loop.run(move |event, event_loop, control_flow| {
        trace!("Received Winit event: {event:?}");

        *control_flow = ControlFlow::Wait;
        match event {
            Event::Resumed => {
                app.resume(event_loop);
            }
            Event::Suspended => {
                trace!("Suspended, dropping render state...");
                app.render_state = None;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_size),
                ..
            } => {
                app.configure_surface_swapchain();
                // Winit: doesn't currently implicitly request a redraw
                // for a resize which may be required on some platforms...
                app.queue_redraw();
            }
            Event::RedrawRequested(_) => {
                trace!("Handling Redraw Request");

                if let Some(ref surface_state) = app.surface_state {
                    if let Some(ref rs) = app.render_state {
                        let frame = surface_state
                            .surface
                            .get_current_texture()
                            .expect("Failed to acquire next swap chain texture");
                        let view = frame
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());
                        let mut encoder =
                            rs.device
                                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                    label: None,
                                });
                        {
                            let mut rpass =
                                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                    label: None,
                                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                        view: &view,
                                        resolve_target: None,
                                        ops: wgpu::Operations {
                                            load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                                            store: true,
                                        },
                                    })],
                                    depth_stencil_attachment: None,
                                });
                            rpass.set_pipeline(&rs.render_pipeline);
                            rpass.draw(0..3, 0..1);
                        }

                        rs.queue.submit(Some(encoder.finish()));
                        frame.present();
                        surface_state.window.request_redraw();
                    }
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    });
}

fn _main(event_loop: EventLoop<()>) {
    run(event_loop);
}

#[allow(dead_code)]
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(android_logger::Config::default().with_min_level(log::Level::Trace));

    let event_loop = EventLoopBuilder::new().with_android_app(app).build();
    _main(event_loop);
}

#[allow(dead_code)]
#[cfg(not(target_os = "android"))]
fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Warn) // Default Log Level
        .parse_default_env()
        .init();

    let event_loop = EventLoopBuilder::new().build();
    _main(event_loop);
}
