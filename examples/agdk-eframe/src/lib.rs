use eframe::egui;
use eframe::{NativeOptions, Renderer};

#[cfg(target_os = "android")]
use android_activity::AndroidApp;

#[derive(Default)]
struct DemoApp {
    demo_windows: egui_demo_lib::DemoWindows
}

impl eframe::App for DemoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.demo_windows.ui(ctx);
    }
}

fn _main(mut options: NativeOptions) {
    options.renderer = Renderer::Wgpu;
    eframe::run_native("My egui App", options, Box::new(|_cc| Box::new(DemoApp::default())));
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;

    android_logger::init_once(android_logger::Config::default().with_min_level(log::Level::Info));

    let mut options = NativeOptions::default();
    options.event_loop_builder = Some(Box::new(move |builder| {
        builder.with_android_app(app);
    }));
    _main(options);
}

#[cfg(not(target_os = "android"))]
fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Warn) // Default Log Level
        .parse_default_env()
        .init();

    _main(NativeOptions::default());
}
