
use log::Level;
use log::trace;
use wgpu::TextureFormat;
use wgpu::{Instance, Adapter, Device, ShaderModule, PipelineLayout, RenderPipeline, Queue};
use winit::event_loop::EventLoopWindowTarget;

use std::ops::Deref;
use std::borrow::Cow;
use std::sync::{Arc, RwLock};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};

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
    surface: wgpu::Surface
}

struct AppInner {
    instance: Instance,
    adapter: Option<Adapter>,
    surface_state: Option<SurfaceState>,
    render_state: Option<RenderState>,
}

struct App {
    inner: Arc<RwLock<AppInner>>
}

impl App {
    fn new(instance: Instance) -> Self {
        Self {
            inner: Arc::new(RwLock::new(AppInner {
                instance,
                adapter: None,
                surface_state: None,
                render_state: None,
            }))
        }
    }
}
impl Deref for App {
    type Target = Arc<RwLock<AppInner>>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
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
    let shader = device.create_shader_module(&wgpu::ShaderModuleDescriptor {
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
            targets: &[target_format.into()],
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

fn configure_surface_swapchain(render_state: &RenderState, surface_state: &SurfaceState) {
    let swapchain_format = render_state.target_format;
    let size = surface_state.window.inner_size();

    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: swapchain_format,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Mailbox,
        //present_mode: wgpu::PresentMode::Fifo,
    };

    trace!("WGPU: Configuring surface swapchain: format = {swapchain_format:?}, size = {size:?}");
    surface_state.surface.configure(&render_state.device, &config);
}

// We want to defer the initialization of our render state until
// we have a surface so we can take its format into account.
//
// After we've initialized our render state once though we
// expect all future surfaces will have the same format and we
// so this stat will remain valid.
async fn ensure_render_state_for_surface(app: &App, new_surface_state: &SurfaceState) {
    let mut app_guard = app.inner.write().unwrap();

    if app_guard.adapter.is_none() {
        trace!("WGPU: requesting a suitable adapter (compatible with our surface)");
        let adapter = app_guard.instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                // Request an adapter which can render to our surface
                compatible_surface: Some(&new_surface_state.surface),
            })
            .await
            .expect("Failed to find an appropriate adapter");

        app_guard.adapter = Some(adapter);
    }
    let adapter = app_guard.adapter.as_ref().unwrap();

    if app_guard.render_state.is_none() {
        trace!("WGPU: finding preferred swapchain format");
        let swapchain_format = new_surface_state.surface.get_preferred_format(&adapter).unwrap();

        let rs = init_render_state(adapter, swapchain_format).await;
        app_guard.render_state = Some(rs);
    }
}

fn create_surface<T>(app: &App, event_loop: &EventLoopWindowTarget<T>) -> SurfaceState {
    let window = winit::window::Window::new(&event_loop).unwrap();

    trace!("WGPU: creating surface for native window");
    let guard = app.inner.read().unwrap();
    let surface = unsafe { guard.instance.create_surface(&window) };

    SurfaceState {
        window,
        surface
    }
}

fn resume<T>(app: &App, event_loop: &EventLoopWindowTarget<T>) {
    trace!("Resumed, creating render state...");

    let new_surface = create_surface(&app, event_loop);

    pollster::block_on(ensure_render_state_for_surface(&app, &new_surface));

    app.write().unwrap().surface_state = Some(new_surface);

    let guard = app.read().unwrap();
    let render_state = guard.render_state.as_ref().unwrap();
    let surface_state = guard.surface_state.as_ref().unwrap();
    configure_surface_swapchain(render_state, surface_state);

    trace!("Making Redraw Request");
    surface_state.window.request_redraw();
}


fn run(event_loop: EventLoop<()>, app: App) {

    //let mut running = false;

    trace!("Running mainloop...");

    event_loop.run(move |event, event_loop, control_flow| {
        trace!("Received Winit event: {event:?}");

        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(winit::event::StartCause::Init) => {
                // Note: that because Winit doesn't currently support lifecycle events consistently
                // across platforms then we effectively issue a fake 'resume' on non-android
                // platforms...
                #[cfg(not(target_os="android"))]
                resume(&app, event_loop)
            }
            Event::Resumed => {
                resume(&app, event_loop);
            }
            Event::Suspended => {
                trace!("Suspended, dropping render state...");
                let mut guard = app.write().unwrap();
                //guard.running = false;
                guard.render_state = None;
            },
            Event::WindowEvent {
                event: WindowEvent::Resized(_size),
                ..
            } => {
                let guard = app.read().unwrap();
                if let Some(ref surface_state) = guard.surface_state {
                    if let Some(ref render_state) = guard.render_state {
                        configure_surface_swapchain(render_state, surface_state);

                        // Winit: doesn't currently implicitly request a redraw
                        // for a resize which may be required on some platforms...
                        surface_state.window.request_redraw();
                    }
                }
            }
            Event::RedrawRequested(_) => {
                trace!("Handling Redraw Request");

                let guard = app.read().unwrap();
                if let Some(ref surface_state) = guard.surface_state {
                    if let Some(ref rs) = guard.render_state {
                        let frame = surface_state.surface
                            .get_current_texture()
                            .expect("Failed to acquire next swap chain texture");
                        let view = frame
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());
                        let mut encoder =
                            rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                        {
                            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: None,
                                color_attachments: &[wgpu::RenderPassColorAttachment {
                                    view: &view,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                                        store: true,
                                    },
                                }],
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


fn _main() {
    let event_loop = EventLoop::new();

    // We can decide on our graphics API / backend up-front and that
    // doesn't need to be re-considered later
    let instance = wgpu::Instance::new(wgpu::Backends::all());
    //let instance = wgpu::Instance::new(wgpu::Backends::VULKAN);
    //let instance = wgpu::Instance::new(wgpu::Backends::GL);

    let app = App::new(instance);

    run(event_loop, app);
}


#[cfg(target_os="android")]
#[no_mangle]
extern "C" fn android_main() {
    android_logger::init_once(
        android_logger::Config::default().with_min_level(Level::Trace)
    );

    _main();
}
// Stop rust-analyzer from complaining that this file doesn't have a main() function...
#[cfg(target_os="android")]
fn main() {}

#[cfg(not(target_os="android"))]
fn main() {
    env_logger::builder().filter_level(log::LevelFilter::Warn) // Default Log Level
        .parse_default_env()
        .init();

    _main();
}