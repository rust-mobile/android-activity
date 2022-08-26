//! An example OpenXR application that draws a head-locked gradient using Wgpu,
//! on top of Vulkan.
//!
//! This builds on code from:
//! https://github.com/Ralith/openxrs/blob/master/openxr/examples/vulkan.rs
//! and
//! https://github.com/zarik5/openxrs/blob/wgpu-test/openxr/examples/vulkan.rs
//! and code from `wgpu_hal::vulkan`
//!

use std::{
    collections::HashSet,
    ffi::{CStr, CString},
    hash::Hash,
    io::Cursor,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::anyhow;
use anyhow::Result;
use bitflags::bitflags;

use ash::{
    util::read_spv,
    vk::{self, Handle},
};

use wgpu_hal as hal;
use wgpu_types as wgt;

use openxr as xr;

#[cfg(target_os = "android")]
use android_activity::AndroidApp;
struct Framebuffer {
    color: wgpu::TextureView,
}

pub struct Swapchain {
    handle: Arc<Mutex<xr::Swapchain<xr::Vulkan>>>,
    buffers: Vec<Framebuffer>,
    resolution: vk::Extent2D,
}

// xr::EnvironmentBlendMode doesn't currently implement Hash
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct XrBlendMode(pub xr::EnvironmentBlendMode);
impl Hash for XrBlendMode {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.into_raw().hash(state);
    }
}

bitflags! {
    #[derive(Default)]
    pub struct PollStatus: u32 {
        const QUIT  = 1<<0;
        const FRAME = 1<<1;
    }
}

pub struct XrShell {
    pub xr_entry: xr::Entry,
    pub xr_instance: xr::Instance,
    pub xr_system: xr::SystemId,
    pub xr_session: xr::Session<xr::vulkan::Vulkan>,

    pub wgpu_adapter: wgpu::Adapter,
    pub wgpu_device: wgpu::Device,
    pub wgpu_queue: wgpu::Queue,

    pub xr_frame_waiter: xr::FrameWaiter,
    pub xr_frame_stream: xr::FrameStream<xr::vulkan::Vulkan>,

    pub xr_blend_modes: HashSet<XrBlendMode>,
    pub xr_current_blend_mode: xr::EnvironmentBlendMode,
    pub xr_swapchain: Swapchain,

    pub xr_event_storage: xr::EventDataBuffer,

    pub quit_signal: Arc<AtomicBool>,
    pub session_running: bool,
}

impl XrShell {
    pub const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;
    pub const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;

    fn hal_instance_flags() -> hal::InstanceFlags {
        let mut flags = hal::InstanceFlags::empty();
        if cfg!(debug_assertions) {
            flags |= hal::InstanceFlags::VALIDATION;

            // WORKAROUND: Requesting the KHR_debug_utils extension on the Oculus Quest fails
            // even though the extension is advertised as being supported!?
            #[cfg(not(target_os = "android"))]
            {
                flags |= hal::InstanceFlags::DEBUG;
            }
        }
        flags
    }

    /// # Safety
    ///
    /// Since wgpu-hal expects a vector of &'static Cstr extensions but we aren't guaranteed to get a 'static
    /// string when querying the required extensions from OpenXR then this function will currently use
    /// `Box::leak()` as a simple way to create static CStrings that can be referenced. The assumption is
    /// that this function is only called once during the lifetime of an application so no effort is made
    /// to share/re-use the 'static boxing between calls.
    ///
    fn create_wgpu_hal_instance_for_openxr(
        xr_instance: &xr::Instance,
        system: xr::SystemId,
        app_name: &str,
        app_version: u32,
        vk_target_version: u32,
        hal_instance_flags: hal::InstanceFlags,
    ) -> Result<(ash::Instance, <hal::api::Vulkan as hal::Api>::Instance)> {
        let entry = unsafe { ash::Entry::load()? };

        let instance_extensions = entry.enumerate_instance_extension_properties(None)?;
        log::info!(
            "All available Vulkan instance extensions: {:?}",
            instance_extensions
        );

        let wgpu_required_instance_extensions =
            <hal::api::Vulkan as hal::Api>::Instance::required_extensions(
                &entry,
                hal_instance_flags,
            )?;
        log::info!(
            "Vulkan instance extensions required by WGPU: {:?}",
            wgpu_required_instance_extensions
        );
        let xr_required_instance_extensions: &'static mut Vec<CString> = Box::leak(Box::new(
            xr_instance
                .vulkan_legacy_instance_extensions(system)?
                .split_ascii_whitespace()
                .map(|s| CString::new(s).unwrap())
                .collect::<Vec<_>>(),
        ));
        log::info!(
            "Vulkan instance extensions required by OpenXR: {:?}",
            xr_required_instance_extensions
        );
        let xr_required_instance_extensions: Vec<&'static CStr> = xr_required_instance_extensions
            .iter()
            .map(|s| s.as_c_str())
            .collect();

        let required_extensions = wgpu_required_instance_extensions
            .iter()
            .chain(xr_required_instance_extensions.iter())
            .map(|s| *s)
            .collect::<Vec<_>>();
        let required_extensions_ptrs = wgpu_required_instance_extensions
            .iter()
            .chain(xr_required_instance_extensions.iter())
            .map(|s| s.as_ptr())
            .collect::<Vec<_>>();

        let driver_api_version = match entry.try_enumerate_instance_version() {
            // Vulkan 1.1+
            Ok(Some(version)) => version,
            Ok(None) => vk::API_VERSION_1_0,
            Err(err) => {
                return Err(anyhow!(
                    "Failed to query supported Vulkan instance version: {:?}",
                    err
                ));
            }
        };

        if driver_api_version < vk_target_version {
            return Err(anyhow!(
                "Vulkan driver version {}.{}.{} less than target version {}.{}.{}",
                vk::api_version_major(driver_api_version),
                vk::api_version_minor(driver_api_version),
                vk::api_version_patch(driver_api_version),
                vk::api_version_major(vk_target_version),
                vk::api_version_minor(vk_target_version),
                vk::api_version_patch(vk_target_version),
            ));
        }

        let app_name = CString::new(app_name).unwrap();
        let app_info = vk::ApplicationInfo::builder()
            .application_name(app_name.as_c_str())
            .application_version(app_version)
            .engine_name(CStr::from_bytes_with_nul(b"wgpu-hal\0").unwrap())
            .engine_version(2)
            .api_version(vk_target_version);

        log::debug!("Enumerating Vulkan instance layer properties");
        let instance_layers = entry.enumerate_instance_layer_properties()?;

        let nv_optimus_layer = CStr::from_bytes_with_nul(b"VK_LAYER_NV_optimus\0").unwrap();
        let has_nv_optimus = instance_layers
            .iter()
            .any(|inst_layer| unsafe { CStr::from_ptr(inst_layer.layer_name.as_ptr()) } == nv_optimus_layer);

        // Check requested layers against the available layers
        let layers = {
            let mut layers: Vec<&'static CStr> = Vec::new();
            if hal_instance_flags.contains(hal::InstanceFlags::VALIDATION) {
                layers.push(CStr::from_bytes_with_nul(b"VK_LAYER_KHRONOS_validation\0").unwrap());
            }

            // Only keep available layers.
            layers.retain(|&layer| {
                if instance_layers.iter().any(
                    |inst_layer| unsafe { CStr::from_ptr(inst_layer.layer_name.as_ptr()) } == layer,
                ) {
                    true
                } else {
                    log::warn!("Unable to find layer: {}", layer.to_string_lossy());
                    false
                }
            });
            layers
        };

        log::debug!("Creating Vulkan instance");
        let vk_instance = {
            let layer_pointers = layers.iter().map(|&s| s.as_ptr()).collect::<Vec<_>>();

            let create_info = vk::InstanceCreateInfo::builder()
                .flags(vk::InstanceCreateFlags::empty())
                .application_info(&app_info)
                .enabled_layer_names(&layer_pointers)
                .enabled_extension_names(&required_extensions_ptrs);

            unsafe { entry.create_instance(&create_info, None)? }
        };

        let android_sdk_version: u32 = {
            #[cfg(target_os = "android")]
            {
                AndroidApp::sdk_version() as u32
            }

            #[cfg(not(target_os = "android"))]
            0
        };

        log::debug!("Creating Wgpu Hal instance");
        let hal_instance = unsafe {
            <hal::api::Vulkan as hal::Api>::Instance::from_raw(
                entry,
                vk_instance.clone(),
                vk_target_version,
                android_sdk_version,
                required_extensions,
                hal_instance_flags,
                has_nv_optimus,
                Some(Box::new(xr_instance.clone())),
            )?
        };

        Ok((vk_instance, hal_instance))
    }

    /// # Safety
    ///
    /// Since wgpu-hal expects a vector of &'static Cstr extensions but we aren't guaranteed to get a 'static
    /// string when querying the required extensions from OpenXR then this function will currently use
    /// `Box::leak()` as a simple way to create static CStrings that can be referenced. The assumption is
    /// that this function is only called once during the lifetime of an application so no effort is made
    /// to share/re-use the 'static boxing between calls.
    ///
    unsafe fn create_wgpu_hal_device_for_openxr(
        xr_instance: &xr::Instance,
        system: xr::SystemId,
        hal_instance: &<hal::api::Vulkan as hal::Api>::Instance,
        vk_instance: &ash::Instance,
        vk_target_version: u32,
        features: wgt::Features,
        limits: &wgt::Limits,
    ) -> (
        vk::PhysicalDevice,
        hal::ExposedAdapter<hal::api::Vulkan>,
        ash::Device,
        hal::OpenDevice<hal::api::Vulkan>,
        u32,
    ) {
        log::debug!("create_wgpu_hal_device_for_openxr");

        let vk_physical_device = vk::PhysicalDevice::from_raw(
            xr_instance
                .vulkan_graphics_device(system, vk_instance.handle().as_raw() as _)
                .unwrap() as _,
        );

        let hal_adapter = hal_instance.expose_adapter(vk_physical_device).unwrap();

        let vk_device_properties = vk_instance.get_physical_device_properties(vk_physical_device);
        if vk_device_properties.api_version < vk_target_version {
            vk_instance.destroy_instance(None);
            panic!("Vulkan physical device doesn't support version 1.1");
        }

        let phd_limits = &vk_device_properties.limits;
        let uab_types = hal::UpdateAfterBindTypes::from_limits(limits, phd_limits);

        let xr_required_device_extensions: &'static mut Vec<CString> = Box::leak(Box::new(
            xr_instance
                .vulkan_legacy_device_extensions(system)
                .unwrap()
                .split_ascii_whitespace()
                .map(|s| CString::new(s).unwrap())
                .collect(),
        ));
        let xr_required_device_extensions: Vec<&CStr> = xr_required_device_extensions
            .iter()
            .map(|s| s.as_c_str())
            .collect();

        let wgpu_required_device_extensions =
            hal_adapter.adapter.required_device_extensions(features);
        let required_device_extensions = xr_required_device_extensions
            .iter()
            .chain(wgpu_required_device_extensions.iter())
            .map(|s| *s)
            .collect::<Vec<_>>();

        let mut enabled_phd_features = hal_adapter.adapter.physical_device_features(
            &required_device_extensions,
            features,
            uab_types,
        );

        let family_index = vk_instance
            .get_physical_device_queue_family_properties(vk_physical_device)
            .into_iter()
            .enumerate()
            .find_map(|(queue_family_index, info)| {
                if info.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                    Some(queue_family_index as u32)
                } else {
                    None
                }
            })
            .expect("Vulkan device has no graphics queue");
        let family_info = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(family_index)
            .queue_priorities(&[1.0])
            .build();
        let family_infos = [family_info];

        let str_pointers = required_device_extensions
            .iter()
            .map(|&s| {
                // Safe because `enabled_extensions` entries have static lifetime.
                s.as_ptr()
            })
            .collect::<Vec<_>>();

        let pre_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(&family_infos)
            .enabled_extension_names(&str_pointers);
        let mut info = enabled_phd_features.add_to_device_create_builder(pre_info);

        // WORKAROUND: wgpu_hal 0.13 omits pushing PhysicalDeviceMultiviewFeatures even `with wgt::Features::MULTIVIEW`
        let mut multiview = vk::PhysicalDeviceMultiviewFeatures {
            multiview: vk::TRUE,
            ..Default::default()
        };
        if features.contains(wgt::Features::MULTIVIEW) {
            info = info.push_next(&mut multiview);
        }

        let vk_device = {
            vk_instance
                .create_device(vk_physical_device, &info.build(), None)
                .unwrap()
        };

        log::debug!("Creating Wgpu Hal device");
        let hal_device = hal_adapter
            .adapter
            .device_from_raw(
                vk_device.clone(),
                true,
                &wgpu_required_device_extensions,
                features,
                uab_types,
                family_info.queue_family_index,
                0,
            )
            .unwrap();

        (
            vk_physical_device,
            hal_adapter,
            vk_device,
            hal_device,
            family_index,
        )
    }

    fn create_swapchain(
        xr_instance: &xr::Instance,
        system: xr::SystemId,
        session: &xr::Session<xr::vulkan::Vulkan>,
        wgpu_device: &wgpu::Device,
    ) -> Result<Swapchain> {
        // Now we need to find all the viewpoints we need to take care of! This is a
        // property of the view configuration type; in this example we use PRIMARY_STEREO,
        // so we should have 2 viewpoints.
        //
        // Because we are using multiview in this example, we require that all view
        // dimensions are identical.
        let views = xr_instance.enumerate_view_configuration_views(system, XrShell::VIEW_TYPE)?;
        assert_eq!(views.len(), 2 as usize);
        assert_eq!(views[0], views[1]);

        // Create a swapchain for the viewpoints! A swapchain is a set of texture buffers
        // used for displaying to screen, typically this is a backbuffer and a front buffer,
        // one for rendering data to, and one for displaying on-screen.
        let resolution = vk::Extent2D {
            width: views[0].recommended_image_rect_width,
            height: views[0].recommended_image_rect_height,
        };
        let handle = session.create_swapchain(&xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                | xr::SwapchainUsageFlags::SAMPLED,
            format: XrShell::COLOR_FORMAT.as_raw() as _,
            // The Vulkan graphics pipeline we create is not set up for multisampling,
            // so we hardcode this to 1. If we used a proper multisampling setup, we
            // could set this to `views[0].recommended_swapchain_sample_count`.
            sample_count: 1,
            width: resolution.width,
            height: resolution.height,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        })?;
        let swapchain = Arc::new(Mutex::new(handle));

        let hal_texture_desc = hal::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: resolution.width,
                height: resolution.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: hal::TextureUses::COLOR_TARGET | hal::TextureUses::RESOURCE,
            memory_flags: hal::MemoryFlags::empty(),
        };

        let texture_desc = wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: resolution.width,
                height: resolution.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        };

        // We'll want to track our own information about the swapchain, so we can draw stuff
        // onto it! We'll also create a buffer for each generated texture here as well.
        let images = swapchain.lock().unwrap().enumerate_images()?;
        unsafe {
            Ok(Swapchain {
                handle: swapchain.clone(),
                resolution,
                buffers: images
                    .into_iter()
                    .map(|color_image| {
                        let color_image = vk::Image::from_raw(color_image);

                        let hal_texture = <hal::api::Vulkan as hal::Api>::Device::texture_from_raw(
                            color_image,
                            &hal_texture_desc,
                            Some(Box::new(swapchain.clone())),
                        );

                        let wgpu_texture = wgpu_device.create_texture_from_hal::<hal::api::Vulkan>(
                            hal_texture,
                            &texture_desc,
                        );

                        let color = wgpu_texture.create_view(&wgpu::TextureViewDescriptor {
                            label: None,
                            format: None,
                            dimension: None,
                            aspect: wgpu::TextureAspect::All,
                            base_mip_level: 0,
                            mip_level_count: None,
                            base_array_layer: 0,
                            array_layer_count: None,
                        });

                        Framebuffer { color }
                    })
                    .collect(),
            })
        }
    }

    fn new(
        app_name: &str,
        app_version: u32,
        vk_target_version: u32,
        features: wgt::Features,
        limits: wgt::Limits,
    ) -> Result<Self> {
        let quit_signal = Arc::new(AtomicBool::new(true));

        let xr_entry = xr::Entry::linked();
        #[cfg(target_os = "android")]
        xr_entry.initialize_android_loader()?;

        let available_extensions = xr_entry.enumerate_extensions()?;
        log::info!("{available_extensions:#?}");

        let mut enabled_extensions = xr::ExtensionSet::default();

        // Note we use the XR_KHR_vulkan_enable extension and _not_
        // XR_KHR_vulkan_enable2 to query the extensions that OpenXR requires.
        // If we were to use XR_KHR_vulkan_enable2 and let OpenXR create the vk
        // instance and device we would have no practical way of knowing what
        // additional extensions OpenXR enables, which would be a problem
        // because we need to inform Wgpu of all the enabled extensions when we
        // use them to create Wgpu resources.
        //
        // Unfortunately the openxrs bindings refers to XR_KHR_vulkan_enable a
        // "legacy" API since it's an older extension but in this case it's the
        // more appropriate choice.
        //
        if available_extensions.khr_vulkan_enable {
            enabled_extensions.khr_vulkan_enable = true;
        } else {
            return Err(anyhow!("Required KHR_vulkan_enable extension missing"));
        }
        #[cfg(target_os = "android")]
        {
            enabled_extensions.khr_android_create_instance = true;
        }

        let xr_instance = xr_entry.create_instance(
            &xr::ApplicationInfo {
                application_name: app_name,
                application_version: app_version,
                engine_name: "XrApp",
                engine_version: 0,
            },
            &enabled_extensions,
            &[],
        )?;

        let instance_props = xr_instance.properties()?;
        log::info!(
            "Loaded OpenXR runtime: {} {}",
            instance_props.runtime_name,
            instance_props.runtime_version
        );

        // Request a form factor from the device (HMD, Handheld, etc.)
        let xr_system = xr_instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)?;

        // Check what blend mode is valid for this device (opaque vs transparent displays). We'll just
        // take the first one available!
        let xr_blend_modes =
            xr_instance.enumerate_environment_blend_modes(xr_system, XrShell::VIEW_TYPE)?;
        if xr_blend_modes.len() == 0 {
            // Not obvious from spec if an empty set would be an error
            return Err(anyhow!("Failed to query XR environment blend modes"));
        }
        let xr_blend_mode = xr_blend_modes[0];

        let xr_blend_modes: HashSet<_> =
            xr_blend_modes.into_iter().map(|m| XrBlendMode(m)).collect();

        // OpenXR wants to ensure apps are using the correct graphics card and Vulkan features and
        // extensions, so the instance and device MUST be set up before Instance::create_session.

        let vk_target_version_xr = xr::Version::new(
            vk::api_version_major(vk_target_version) as u16,
            vk::api_version_minor(vk_target_version) as u16,
            vk::api_version_patch(vk_target_version),
        );

        let reqs = xr_instance.graphics_requirements::<xr::Vulkan>(xr_system)?;

        if vk_target_version_xr < reqs.min_api_version_supported
            || vk_target_version_xr.major() > reqs.max_api_version_supported.major()
        {
            return Err(anyhow!(
                "OpenXR runtime requires Vulkan version > {}, < {}.0.0",
                reqs.min_api_version_supported,
                reqs.max_api_version_supported.major() + 1
            ));
        }

        unsafe {
            let (vk_instance, hal_instance) = Self::create_wgpu_hal_instance_for_openxr(
                &xr_instance,
                xr_system,
                app_name,
                app_version,
                vk_target_version,
                Self::hal_instance_flags(),
            )?;

            let (vk_physical_device, hal_adapter, vk_device, hal_device, queue_family_index) =
                Self::create_wgpu_hal_device_for_openxr(
                    &xr_instance,
                    xr_system,
                    &hal_instance,
                    &vk_instance,
                    vk_target_version,
                    features,
                    &limits,
                );

            let wgpu_instance = wgpu::Instance::from_hal::<hal::api::Vulkan>(hal_instance);
            let wgpu_adapter = wgpu_instance.create_adapter_from_hal(hal_adapter);
            let (wgpu_device, wgpu_queue) = wgpu_adapter.create_device_from_hal(
                hal_device,
                &wgpu::DeviceDescriptor {
                    label: None,
                    features,
                    limits,
                },
                None,
            )?;

            // A session represents this application's desire to display things! This is where we hook
            // up our graphics API. This does not start the session; for that, you'll need a call to
            // Session::begin, which we do in the main loop.
            let (xr_session, xr_frame_waiter, xr_frame_stream) = xr_instance
                .create_session::<xr::Vulkan>(
                    xr_system,
                    &xr::vulkan::SessionCreateInfo {
                        instance: vk_instance.handle().as_raw() as _,
                        physical_device: vk_physical_device.as_raw() as _,
                        device: vk_device.handle().as_raw() as _,
                        queue_family_index,
                        queue_index: 0,
                    },
                )?;

            let xr_swapchain =
                Self::create_swapchain(&xr_instance, xr_system, &xr_session, &wgpu_device)?;

            let event_storage = xr::EventDataBuffer::new();
            let session_running = false;

            Ok(Self {
                xr_entry,
                xr_instance,
                xr_system,
                xr_session,

                wgpu_adapter,
                wgpu_device,
                wgpu_queue,

                xr_frame_waiter,
                xr_frame_stream,

                xr_blend_modes,
                xr_current_blend_mode: xr_blend_mode,
                xr_swapchain,
                xr_event_storage: event_storage,

                quit_signal,
                session_running,
            })
        }
    }

    pub fn poll_events(&mut self) -> Result<PollStatus> {
        log::info!("Poll Events");
        // Index of the current frame, wrapped by PIPELINE_DEPTH. Not to be confused with the
        // swapchain image index.
        if !self.quit_signal.load(Ordering::Relaxed) {
            log::debug!("requesting exit");
            // The OpenXR runtime may want to perform a smooth transition between scenes, so we
            // can't necessarily exit instantly. Instead, we must notify the runtime of our
            // intent and wait for it to tell us when we're actually done.
            match self.xr_session.request_exit() {
                Ok(()) => {}
                Err(xr::sys::Result::ERROR_SESSION_NOT_RUNNING) => return Ok(PollStatus::QUIT),
                Err(e) => return Err(anyhow!("{}", e)),
            }
        }

        let mut status = PollStatus::FRAME;

        while let Some(event) = self
            .xr_instance
            .poll_event(&mut self.xr_event_storage)
            .unwrap()
        {
            use xr::Event::*;
            match event {
                SessionStateChanged(e) => {
                    // Session state change is where we can begin and end sessions, as well as
                    // find quit messages!
                    log::info!("entered state {:?}", e.state());
                    match e.state() {
                        xr::SessionState::READY => {
                            self.xr_session.begin(XrShell::VIEW_TYPE).unwrap();
                            self.session_running = true;
                        }
                        xr::SessionState::STOPPING => {
                            self.xr_session.end().unwrap();
                            self.session_running = false;
                            status.set(PollStatus::FRAME, false);
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            status.set(PollStatus::FRAME, false);
                            status.set(PollStatus::QUIT, true);
                        }
                        _ => {}
                    }
                }
                InstanceLossPending(_) => {
                    status.set(PollStatus::FRAME, false);
                    status.set(PollStatus::QUIT, true);
                }
                EventsLost(e) => {
                    log::error!("lost {} events", e.lost_event_count());
                }
                _ => {}
            }
        }

        if !self.session_running {
            // Don't grind up the CPU
            std::thread::sleep(Duration::from_millis(100));
            status.set(PollStatus::FRAME, false);
        }

        Ok(status)
    }
}

struct App {
    xr_shell: XrShell,

    xr_action_set: xr::ActionSet,
    xr_left_action: xr::Action<xr::Posef>,
    xr_right_action: xr::Action<xr::Posef>,
    xr_left_space: xr::Space,
    xr_right_space: xr::Space,
    xr_stage: xr::Space,

    wgpu_render_pipeline: wgpu::RenderPipeline,
}

impl App {
    fn new() -> Result<Self> {
        let vk_target_version = vk::make_api_version(0, 1, 1, 0); // Vulkan 1.1 guarantees multiview support

        let features = wgpu::Features::SPIRV_SHADER_PASSTHROUGH | wgt::Features::MULTIVIEW;
        let limits = wgt::Limits::default();

        let xr_session = XrShell::new("OpenXR Wgpu", 1, vk_target_version, features, limits)?;

        let vertex_shader = unsafe {
            xr_session
                .wgpu_device
                .create_shader_module_spirv(&wgpu::ShaderModuleDescriptorSpirV {
                    label: None,
                    source: read_spv(&mut Cursor::new(&include_bytes!("fullscreen.vert.spv")[..]))?
                        .into(),
                })
        };
        let fragment_shader = unsafe {
            xr_session
                .wgpu_device
                .create_shader_module_spirv(&wgpu::ShaderModuleDescriptorSpirV {
                    label: None,
                    source: read_spv(&mut Cursor::new(
                        &include_bytes!("debug_pattern.frag.spv")[..],
                    ))?
                    .into(),
                })
        };

        let pipeline_layout =
            xr_session
                .wgpu_device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: None,
                    bind_group_layouts: &[],
                    push_constant_ranges: &[],
                });

        let wgpu_render_pipeline =
            xr_session
                .wgpu_device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &vertex_shader,
                        entry_point: "main",
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        unclipped_depth: false,
                        polygon_mode: wgpu::PolygonMode::Fill,
                        conservative: false,
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState {
                        count: 1,
                        mask: !0x0,
                        alpha_to_coverage_enabled: false,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &fragment_shader,
                        entry_point: "main",
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba8UnormSrgb,
                            blend: None,
                            write_mask: wgpu::ColorWrites::RED
                                | wgpu::ColorWrites::GREEN
                                | wgpu::ColorWrites::BLUE,
                        })],
                    }),
                    multiview: None,
                });

        // Create an action set to encapsulate our actions
        let xr_action_set =
            xr_session
                .xr_instance
                .create_action_set("input", "input pose information", 0)?;

        let xr_right_action =
            xr_action_set.create_action::<xr::Posef>("right_hand", "Right Hand Controller", &[])?;
        let xr_left_action =
            xr_action_set.create_action::<xr::Posef>("left_hand", "Left Hand Controller", &[])?;

        // Bind our actions to input devices using the given profile
        // If you want to access inputs specific to a particular device you may specify a different
        // interaction profile
        xr_session
            .xr_instance
            .suggest_interaction_profile_bindings(
                xr_session
                    .xr_instance
                    .string_to_path("/interaction_profiles/khr/simple_controller")?,
                &[
                    xr::Binding::new(
                        &xr_right_action,
                        xr_session
                            .xr_instance
                            .string_to_path("/user/hand/right/input/grip/pose")?,
                    ),
                    xr::Binding::new(
                        &xr_left_action,
                        xr_session
                            .xr_instance
                            .string_to_path("/user/hand/left/input/grip/pose")?,
                    ),
                ],
            )?;

        // Attach the action set to the session
        xr_session
            .xr_session
            .attach_action_sets(&[&xr_action_set])
            .unwrap();

        // Create an action space for each device we want to locate
        let xr_right_space = xr_right_action.create_space(
            xr_session.xr_session.clone(),
            xr::Path::NULL,
            xr::Posef::IDENTITY,
        )?;
        let xr_left_space = xr_left_action.create_space(
            xr_session.xr_session.clone(),
            xr::Path::NULL,
            xr::Posef::IDENTITY,
        )?;

        // OpenXR uses a couple different types of reference frames for positioning content; we need
        // to choose one for displaying our content! STAGE would be relative to the center of your
        // guardian system's bounds, and LOCAL would be relative to your device's starting location.
        let xr_stage = xr_session
            .xr_session
            .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)?;

        Ok(Self {
            xr_shell: xr_session,

            xr_action_set,
            xr_left_action,
            xr_right_action,
            xr_left_space,
            xr_right_space,
            xr_stage,

            wgpu_render_pipeline,
        })
    }

    pub fn poll_events(&mut self) -> Result<PollStatus> {
        self.xr_shell.poll_events()
    }

    pub fn frame_update(&mut self) -> Result<()> {
        // Block until the previous frame is finished displaying, and is ready for another one.
        // Also returns a prediction of when the next frame will be displayed, for use with
        // predicting locations of controllers, viewpoints, etc.
        let frame_state = self.xr_shell.xr_frame_waiter.wait()?;

        // Spec: "An application must eventually match each xrWaitFrame call with one call to xrBeginFrame"
        self.xr_shell.xr_frame_stream.begin()?;

        // Id would be nice if we could consistently end() the frame here to be clear about
        // the 1:1 relationship begin() and end() but we can't (practically) return the
        // slice of layers from the render function and so we rely on render() calling
        // frame_stream.end() before it returns.

        let (rendered, render_status) = if frame_state.should_render {
            (true, self.render(frame_state))
        } else {
            (false, Ok(()))
        };

        // Spec: "Every application must call xrBeginFrame before calling xrEndFrame, and should call
        //  xrEndFrame before calling xrBeginFrame again."
        if !rendered || !render_status.is_ok() {
            self.xr_shell.xr_frame_stream.end(
                frame_state.predicted_display_time,
                self.xr_shell.xr_current_blend_mode,
                &[],
            )?;
        };

        render_status
    }

    pub fn render(&mut self, frame_state: xr::FrameState) -> Result<()> {
        log::info!("Render");
        debug_assert!(frame_state.should_render);

        // We need to ask which swapchain image to use for rendering! Which one will we get?
        // Who knows! It's up to the runtime to decide.
        let image_index = self
            .xr_shell
            .xr_swapchain
            .handle
            .lock()
            .unwrap()
            .acquire_image()?;

        // Wait until the image is available to render to. The compositor could still be
        // reading from it.
        self.xr_shell
            .xr_swapchain
            .handle
            .lock()
            .unwrap()
            .wait_image(xr::Duration::INFINITE)?;

        let mut command_encoder = self
            .xr_shell
            .wgpu_device
            .create_command_encoder(&Default::default());

        let mut render_pass = command_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.xr_shell.xr_swapchain.buffers[image_index as usize].color,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });

        render_pass.set_viewport(
            0_f32,
            0_f32,
            self.xr_shell.xr_swapchain.resolution.width as _,
            self.xr_shell.xr_swapchain.resolution.width as _,
            0_f32,
            1_f32,
        );
        render_pass.set_scissor_rect(
            0,
            0,
            self.xr_shell.xr_swapchain.resolution.width,
            self.xr_shell.xr_swapchain.resolution.width,
        );

        render_pass.set_pipeline(&self.wgpu_render_pipeline);
        render_pass.draw(0..3, 0..1);
        drop(render_pass);

        let command_buffer = command_encoder.finish();

        self.xr_shell
            .xr_session
            .sync_actions(&[(&self.xr_action_set).into()])
            .unwrap();

        // Find where our controllers are located in the Stage space
        let right_location = self
            .xr_right_space
            .locate(&self.xr_stage, frame_state.predicted_display_time)
            .unwrap();

        let left_location = self
            .xr_left_space
            .locate(&self.xr_stage, frame_state.predicted_display_time)
            .unwrap();

        let mut printed = false;
        if self
            .xr_left_action
            .is_active(&self.xr_shell.xr_session, xr::Path::NULL)
            .unwrap()
        {
            print!(
                "Left Hand: ({:0<12},{:0<12},{:0<12}), ",
                left_location.pose.position.x,
                left_location.pose.position.y,
                left_location.pose.position.z
            );
            printed = true;
        }

        if self
            .xr_right_action
            .is_active(&self.xr_shell.xr_session, xr::Path::NULL)
            .unwrap()
        {
            print!(
                "Right Hand: ({:0<12},{:0<12},{:0<12})",
                right_location.pose.position.x,
                right_location.pose.position.y,
                right_location.pose.position.z
            );
            printed = true;
        }
        if printed {
            println!();
        }

        // Fetch the view transforms. To minimize latency, we intentionally do this *after*
        // recording commands to render the scene, i.e. at the last possible moment before
        // rendering begins in earnest on the GPU. Uniforms dependent on this data can be sent
        // to the GPU just-in-time by writing them to per-frame host-visible memory which the
        // GPU will only read once the command buffer is submitted.
        let (_, views) = self.xr_shell.xr_session.locate_views(
            XrShell::VIEW_TYPE,
            frame_state.predicted_display_time,
            &self.xr_stage,
        )?;

        self.xr_shell.wgpu_queue.submit(Some(command_buffer));
        self.xr_shell
            .xr_swapchain
            .handle
            .lock()
            .unwrap()
            .release_image()?;

        // Tell OpenXR what to present for this frame
        let rect = xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                width: self.xr_shell.xr_swapchain.resolution.width as _,
                height: self.xr_shell.xr_swapchain.resolution.height as _,
            },
        };

        let swapchain = &self.xr_shell.xr_swapchain.handle.lock().unwrap();

        self.xr_shell.xr_frame_stream.end(
            frame_state.predicted_display_time,
            self.xr_shell.xr_current_blend_mode,
            &[&xr::CompositionLayerProjection::new()
                .space(&self.xr_stage)
                .views(&[
                    xr::CompositionLayerProjectionView::new()
                        .pose(views[0].pose)
                        .fov(views[0].fov)
                        .sub_image(
                            xr::SwapchainSubImage::new()
                                .swapchain(swapchain)
                                .image_array_index(0)
                                .image_rect(rect),
                        ),
                    xr::CompositionLayerProjectionView::new()
                        .pose(views[1].pose)
                        .fov(views[1].fov)
                        .sub_image(
                            xr::SwapchainSubImage::new()
                                .swapchain(swapchain)
                                .image_array_index(0)
                                .image_rect(rect),
                        ),
                ])],
        )?;

        Ok(())
    }
}

#[allow(dead_code)]
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(android_app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_min_level(log::Level::Trace));

    let mut app = App::new().unwrap();

    log::trace!("Running mainloop...");
    'mainloop: loop {
        android_app.poll_events(Some(Duration::from_secs(0)), |event| {
            log::info!("Android event {:?}", event);
        });

        let status = app.poll_events().unwrap();

        if status.contains(PollStatus::QUIT) {
            log::info!("Mainloop Quitting");
            break 'mainloop;
        }

        if status.contains(PollStatus::FRAME) {
            app.frame_update().unwrap();
        }
    }
}

#[allow(dead_code)]
#[cfg(not(target_os = "android"))]
fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .parse_default_env()
        .init();

    let mut app = App::new().unwrap();

    let r = app.xr_shell.quit_signal.clone();
    let _ = ctrlc::set_handler(move || {
        r.store(false, Ordering::Relaxed);
    });

    log::trace!("Running mainloop...");
    'mainloop: loop {
        let status = app.poll_events()?;

        if status.contains(PollStatus::QUIT) {
            log::info!("Mainloop Quitting");
            break 'mainloop;
        }

        if status.contains(PollStatus::FRAME) {
            app.frame_update()?;
        }
    }

    Ok(())
}
