
use log::Level;
use log::{trace, warn, error};
use wgpu::TextureFormat;
use wgpu::{Instance, Adapter, Device, Queue};
use winit::event_loop::{EventLoopWindowTarget, EventLoopBuilder};

use std::ops::Deref;
use std::sync::{Arc, RwLock};
use winit::{
    event_loop::{ControlFlow},
};

use std::iter;
use std::time::Instant;

use chrono::Timelike;
use egui::FontDefinitions;
use egui_wgpu_backend::{RenderPass, ScreenDescriptor};
use egui_winit_platform::{Platform, PlatformDescriptor};
use epi::{*, App};
use winit::event::Event::*;
const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;



struct RenderState {
    device: Device,
    queue: Queue,
    target_format: TextureFormat,
    egui_rpass: RwLock<RenderPass>,
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

struct EguiApp {
    inner: Arc<RwLock<AppInner>>
}

impl EguiApp {
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
impl Deref for EguiApp {
    type Target = Arc<RwLock<AppInner>>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}


/// A custom event type for the winit app.
enum Event {
    RequestRedraw,
}

/// This is the repaint signal type that egui needs for requesting a repaint from another thread.
/// It sends the custom RequestRedraw event to the winit event loop.
struct ExampleRepaintSignal(std::sync::Mutex<winit::event_loop::EventLoopProxy<Event>>);

impl epi::backend::RepaintSignal for ExampleRepaintSignal {
    fn request_repaint(&self) {
        self.0.lock().unwrap().send_event(Event::RequestRedraw).ok();
    }
}


/// Time of day as seconds since midnight. Used for clock in demo app.
pub fn seconds_since_midnight() -> f64 {
    let time = chrono::Local::now().time();
    time.num_seconds_from_midnight() as f64 + 1e-9 * (time.nanosecond() as f64)
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

    let egui_rpass = RenderPass::new(&device, target_format, 1);

    RenderState {
        device,
        queue,
        target_format,
        egui_rpass: RwLock::new(egui_rpass),
    }
}

fn configure_surface_swapchain(render_state: &RenderState, surface_state: &SurfaceState) {
    let swapchain_format = render_state.target_format;
    let size = surface_state.window.inner_size();

    let config = wgpu::SurfaceConfiguration {
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
async fn ensure_render_state_for_surface(app: &EguiApp, new_surface_state: &SurfaceState) {
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

fn create_surface<T>(app: &EguiApp, event_loop: &EventLoopWindowTarget<T>) -> SurfaceState {
    //let window = winit::window::Window::new(&event_loop).unwrap();
    let window = winit::window::WindowBuilder::new()
        .with_decorations(true)
        .with_resizable(true)
        .with_transparent(false)
        .with_title("egui-wgpu_winit example")
        .with_inner_size(winit::dpi::PhysicalSize {
            width: INITIAL_WIDTH,
            height: INITIAL_HEIGHT,
        })
            .build(&event_loop)
            .unwrap();
    trace!("WGPU: creating surface for native window");
    let guard = app.inner.read().unwrap();
    let surface = unsafe { guard.instance.create_surface(&window) };

    SurfaceState {
        window,
        surface
    }
}

fn resume<T>(app: &EguiApp, event_loop: &EventLoopWindowTarget<T>, platform: &mut Platform) {
    trace!("Resumed, creating render state...");

    let new_surface = create_surface(&app, event_loop);

    pollster::block_on(ensure_render_state_for_surface(&app, &new_surface));

    app.write().unwrap().surface_state = Some(new_surface);

    let guard = app.read().unwrap();
    let render_state = guard.render_state.as_ref().unwrap();
    let surface_state = guard.surface_state.as_ref().unwrap();
    configure_surface_swapchain(render_state, surface_state);

    let mut size = surface_state.window.inner_size();
    let scale_factor = surface_state.window.scale_factor() as f32;
    let raw_input = platform.raw_input_mut();
    raw_input.pixels_per_point = Some(scale_factor);
    raw_input.screen_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::default(),
        egui::vec2(
            size.width as f32,
            size.height as f32,
        ) / scale_factor,
    ));

    // WORKAROUND
    //
    // egui_winit_platform maintains a private scale_factor that's normally
    // only possible to initialize via `Platform::new()` which isn't possible
    // on Android before we have a Window. Currently the only way we can update this
    // state is by issuing a fake ScaleFactorChanged event...
    //
    // See: https://github.com/hasenbanck/egui_winit_platform/issues/40
    platform.handle_event::<Event>(&winit::event::Event::WindowEvent {
        window_id: surface_state.window.id(),
        event: winit::event::WindowEvent::ScaleFactorChanged {
            scale_factor: scale_factor as f64,
            new_inner_size: &mut size }
    });

    trace!("Making Redraw Request");
    surface_state.window.request_redraw();
}

fn _main() {
    let instance = wgpu::Instance::new(wgpu::Backends::PRIMARY);
    //let instance = wgpu::Instance::new(wgpu::Backends::VULKAN);
    //let instance = wgpu::Instance::new(wgpu::Backends::GL);

    let app = EguiApp::new(instance);

    let event_loop = EventLoopBuilder::with_user_event().build();

    let repaint_signal = std::sync::Arc::new(ExampleRepaintSignal(std::sync::Mutex::new(
        event_loop.create_proxy(),
    )));

    // We use the egui_winit_platform crate as the platform.
    // Note: we are initializing with dummy state because we don't technically know
    // anything about the physical size or required scale factor yet...
    let mut platform = Platform::new(PlatformDescriptor {
        physical_width: INITIAL_WIDTH,
        physical_height: INITIAL_HEIGHT,
        scale_factor: 1.0,
        font_definitions: FontDefinitions::default(),
        style: Default::default(),
    });

    // We use the egui_wgpu_backend crate as the render backend.

    // Display the demo application that ships with egui.
    let mut demo_app = egui_demo_lib::WrapApp::default();

    let start_time = Instant::now();
    let mut previous_frame_time = None;
    event_loop.run(move |event, event_loop, control_flow| {
        // Pass the winit events to the platform integration.
        platform.handle_event(&event);

        match event {
            NewEvents(winit::event::StartCause::Init) => {
                // Note: that because Winit doesn't currently support lifecycle events consistently
                // across platforms then we effectively issue a fake 'resume' on non-android
                // platforms...
                #[cfg(not(target_os="android"))]
                resume(&app, event_loop, &mut platform)
            }
            Resumed => {
                resume(&app, event_loop, &mut platform);
            }
            RedrawRequested(..) => {
                let guard = app.read().unwrap();

                if let Some(ref surface_state) = guard.surface_state {
                    if let Some(ref render_state) = guard.render_state {
                        platform.update_time(start_time.elapsed().as_secs_f64());

                        let size = surface_state.window.inner_size();
                        let scale_factor = surface_state.window.scale_factor() as f32;

                        let output_frame = match surface_state.surface.get_current_texture() {
                            Ok(frame) => frame,
                            Err(wgpu::SurfaceError::Outdated) => {
                                // This error occurs when the app is minimized on Windows.
                                // Silently return here to prevent spamming the console with:
                                // "The underlying surface has changed, and therefore the swap chain must be updated"
                                return;
                            }
                            Err(e) => {
                                eprintln!("Dropped frame with error: {}", e);
                                return;
                            }
                        };
                        let output_view = output_frame
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        // Begin to draw the UI frame.
                        let egui_start = Instant::now();
                        platform.begin_frame();
                        let app_output = epi::backend::AppOutput::default();

                        let mut frame =  epi::Frame::new(epi::backend::FrameData {
                            info: epi::IntegrationInfo {
                                name: "egui_example",
                                web_info: None,
                                cpu_usage: previous_frame_time,
                                native_pixels_per_point: Some(scale_factor),
                                prefer_dark_mode: None,
                            },
                            output: app_output,
                            repaint_signal: repaint_signal.clone(),
                        });

                        // Draw the demo application.
                        demo_app.update(&platform.context(), &mut frame);

                        // End the UI frame. We could now handle the output and draw the UI with the backend.
                        //let (_output, paint_commands) =
                        let output = platform.end_frame(Some(&surface_state.window));
                        let paint_jobs = platform.context().tessellate(output.shapes);

                        let frame_time = (Instant::now() - egui_start).as_secs_f64() as f32;
                        previous_frame_time = Some(frame_time);

                        let mut encoder = render_state.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("encoder"),
                        });

                        // Upload all resources for the GPU.
                        let screen_descriptor = ScreenDescriptor {
                            physical_width: size.width,
                            physical_height: size.height,
                            scale_factor: scale_factor,
                        };

                        {
                            let mut egui_rpass = render_state.egui_rpass.write().unwrap();
                            match egui_rpass.add_textures(&render_state.device, &render_state.queue, &output.textures_delta) {
                                Ok(()) => {
                                    //egui_rpass.update_texture(&device, &queue, &platform.context().font_image());
                                    //egui_rpass.update_user_textures(&device, &queue);
                                    egui_rpass.update_buffers(&render_state.device, &render_state.queue, &paint_jobs, &screen_descriptor);

                                    // Record all render passes.
                                    egui_rpass
                                        .execute(
                                            &mut encoder,
                                            &output_view,
                                            &paint_jobs,
                                            &screen_descriptor,
                                            Some(wgpu::Color::BLACK),
                                        )
                                        .unwrap();
                                    // Submit the commands.
                                    render_state.queue.submit(iter::once(encoder.finish()));

                                    // Redraw egui
                                    output_frame.present();

                                    if let Err(err) = egui_rpass.remove_textures(output.textures_delta) {
                                        error!("Failed to remove texture deltas from Egui render pass: {err:?}");
                                    }
                                }
                                Err(err) => {
                                    error!("Failed to add texture deltas before executing render pass: {err:?}");
                                }
                            }
                        }

                        // Suppport reactive on windows only, but not on linux.
                        // if _output.needs_repaint {
                        //     *control_flow = ControlFlow::Poll;
                        // } else {
                        //     *control_flow = ControlFlow::Wait;
                        // }
                    }
                }
            }
            MainEventsCleared | UserEvent(Event::RequestRedraw) => {
                let guard = app.read().unwrap();
                if let Some(ref surface_state) = guard.surface_state {
                    surface_state.window.request_redraw();
                }
            }
            WindowEvent { event, .. } => match event {
                winit::event::WindowEvent::Resized(size) => {

                    let guard = app.read().unwrap();
                    if let Some(ref surface_state) = guard.surface_state {
                        if let Some(ref render_state) = guard.render_state {
                            warn!("resized size = {size:?}, window inner_size = {:?}", surface_state.window.inner_size());
                            // BUG WORKAROUND:
                            //
                            // Currently Windows will emit an erroneous Resized 0x0 event on minimize which doesn't
                            // even logically make sense on Windows, where apps can continue to render for minimization
                            // thumbnails. A 0x0 size will almost certainly lead to divide zeros when calculating
                            // aspect ratios so we just ignore the event.
                            //
                            // See: https://github.com/rust-windowing/winit/issues/208
                            if size.width > 0 && size.height > 0 {
                                configure_surface_swapchain(render_state, surface_state);
                            }
                        }
                    }
                }
                winit::event::WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            },
            _ => (),
        }
    });
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
#[cfg(allow_unused)]
fn main() {}

#[cfg(not(target_os="android"))]
fn main() {
    env_logger::builder().filter_level(log::LevelFilter::Warn) // Default Log Level
        .parse_default_env()
        .init();

    _main();
}