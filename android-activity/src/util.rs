use jni::{
    jni_sig, jni_str,
    objects::{JObject, JString, JThread},
    vm::JavaVM,
};
use log::{error, Level};
use ndk::asset::AssetManager;
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{BufRead as _, BufReader, Result},
    os::{
        fd::{FromRawFd as _, RawFd},
        raw::c_char,
    },
    sync::OnceLock,
};

use crate::main_callbacks::MainCallbacks;

pub fn try_get_path_from_ptr(path: *const c_char) -> Option<std::path::PathBuf> {
    if path.is_null() {
        return None;
    }
    let cstr = unsafe {
        let cstr_slice = CStr::from_ptr(path.cast());
        cstr_slice.to_str().ok()?
    };
    if cstr.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(cstr))
}

pub(crate) fn android_log(level: Level, tag: &CStr, msg: &CStr) {
    let prio = match level {
        Level::Error => ndk_sys::android_LogPriority::ANDROID_LOG_ERROR,
        Level::Warn => ndk_sys::android_LogPriority::ANDROID_LOG_WARN,
        Level::Info => ndk_sys::android_LogPriority::ANDROID_LOG_INFO,
        Level::Debug => ndk_sys::android_LogPriority::ANDROID_LOG_DEBUG,
        Level::Trace => ndk_sys::android_LogPriority::ANDROID_LOG_VERBOSE,
    };
    unsafe {
        ndk_sys::__android_log_write(prio.0 as libc::c_int, tag.as_ptr(), msg.as_ptr());
    }
}

pub(crate) fn forward_stdio_to_logcat() -> std::thread::JoinHandle<Result<()>> {
    // XXX: make this stdout/stderr redirection an optional / opt-in feature?...

    let file = unsafe {
        let mut logpipe: [RawFd; 2] = Default::default();
        libc::pipe2(logpipe.as_mut_ptr(), libc::O_CLOEXEC);
        libc::dup2(logpipe[1], libc::STDOUT_FILENO);
        libc::dup2(logpipe[1], libc::STDERR_FILENO);
        libc::close(logpipe[1]);

        File::from_raw_fd(logpipe[0])
    };

    std::thread::Builder::new()
        .name("stdio-to-logcat".to_string())
        .spawn(move || -> Result<()> {
            let tag = CStr::from_bytes_with_nul(b"RustStdoutStderr\0").unwrap();
            let mut reader = BufReader::new(file);
            let mut buffer = String::new();
            loop {
                buffer.clear();
                let len = match reader.read_line(&mut buffer) {
                    Ok(len) => len,
                    Err(e) => {
                        error!("Logcat forwarder failed to read stdin/stderr: {e:?}");
                        break Err(e);
                    }
                };
                if len == 0 {
                    break Ok(());
                } else if let Ok(msg) = CString::new(buffer.clone()) {
                    android_log(Level::Info, tag, &msg);
                }
            }
        })
        .expect("Failed to start stdout/stderr to logcat forwarder thread")
}

pub(crate) fn log_panic(panic: Box<dyn std::any::Any + Send>) {
    let rust_panic = unsafe { CStr::from_bytes_with_nul_unchecked(b"RustPanic\0") };

    if let Some(panic) = panic.downcast_ref::<String>() {
        if let Ok(msg) = CString::new(panic.clone()) {
            android_log(Level::Error, rust_panic, &msg);
        }
    } else if let Ok(panic) = panic.downcast::<&str>() {
        if let Ok(msg) = CString::new(*panic) {
            android_log(Level::Error, rust_panic, &msg);
        }
    } else {
        let unknown_panic = unsafe { CStr::from_bytes_with_nul_unchecked(b"UnknownPanic\0") };
        android_log(Level::Error, unknown_panic, unsafe {
            CStr::from_bytes_with_nul_unchecked(b"\0")
        });
    }
}

/// Run a closure and abort the program if it panics.
///
/// This is generally used to ensure Rust callbacks won't unwind past the JNI boundary, which leads
/// to undefined behaviour.
///
/// TODO(rib): throw a Java exception instead of aborting. An Android Activity does not necessarily
/// own the entire process because other application Services (or even Activities) may run in
/// threads within the same process, and so we're tearing down too much by aborting the process.
pub(crate) fn abort_on_panic<R>(f: impl FnOnce() -> R) -> R {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|panic| {
        // Try logging the panic before aborting
        //
        // Just in case our attempt to log a panic could itself cause a panic we use a
        // second catch_unwind here.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| log_panic(panic)));
        std::process::abort();
    })
}

struct AppState {
    main_callbacks: MainCallbacks,
    app_asset_manager: AssetManager,
}

static APP_ONCE: OnceLock<AppState> = OnceLock::new();

// Get the Application instance from the Activity
pub(crate) fn get_application<'local, 'any>(
    env: &mut jni::Env<'local>,
    activity: &JObject<'any>,
) -> jni::errors::Result<JObject<'local>> {
    let app = env
        .call_method(
            activity,
            jni_str!("getApplication"),
            jni_sig!(() -> android.app.Application),
            &[],
        )?
        .l()?;
    Ok(app)
}

pub(crate) fn get_assets<'local, 'any>(
    env: &mut jni::Env<'local>,
    application: &JObject<'any>,
) -> jni::errors::Result<JObject<'local>> {
    let assets_manager = env
        .call_method(
            application,
            jni_str!("getAssets"),
            jni_sig!(() -> android.content.res.AssetManager),
            &[],
        )?
        .l()?;
    Ok(assets_manager)
}

fn try_init_current_thread(env: &mut jni::Env, activity: &JObject) -> jni::errors::Result<()> {
    let activity_class = env.get_object_class(activity)?;
    let class_loader = activity_class.get_class_loader(env)?;

    let thread = JThread::current_thread(env)?;
    thread.set_context_class_loader(env, &class_loader)?;
    let thread_name = JString::from_jni_str(env, jni_str!("android_main"))?;
    thread.set_name(env, &thread_name)?;

    // Also name native thread - this needs to happen here after attaching to a JVM thread,
    // since that changes the thread name to something like "Thread-2".
    unsafe {
        let thread_name = std::ffi::CStr::from_bytes_with_nul(b"android_main\0").unwrap();
        let _ = libc::pthread_setname_np(libc::pthread_self(), thread_name.as_ptr());
    }
    Ok(())
}

/// Name the Java Thread + native thread "android_main" and set the Java Thread context class loader
/// so that jni code can more-easily find non-system Java classes.
pub(crate) fn init_android_main_thread(
    vm: &JavaVM,
    jni_activity: &JObject,
    java_main_looper: &ndk::looper::ForeignLooper,
) -> jni::errors::Result<(AssetManager, MainCallbacks)> {
    vm.with_local_frame(10, |env| -> jni::errors::Result<_> {
        let app_state = APP_ONCE.get_or_init(|| unsafe {
            let application =
                get_application(env, jni_activity).expect("Failed to get Application instance");
            let app_asset_manager =
                get_assets(env, &application).expect("Failed to get AssetManager");
            let app_global = env
                .new_global_ref(application)
                .expect("Failed to create global ref for Application");
            // Make sure we don't delete the global reference via Drop
            let app_global = app_global.into_raw();
            ndk_context::initialize_android_context(vm.get_raw().cast(), app_global.cast());

            let asset_manager_global = env
                .new_global_ref(app_asset_manager)
                .expect("Failed to create global ref for AssetManager");
            // Make sure we don't delete the global reference via Drop because
            // the AAssetManager pointer will only be valid while we can
            // guarantee that the Java AssetManager is not garbage collected
            let asset_manager_global = asset_manager_global.into_raw();
            let asset_manager_ptr =
                ndk_sys::AAssetManager_fromJava(env.get_raw() as _, asset_manager_global as _);
            assert_ne!(
                asset_manager_ptr,
                std::ptr::null_mut(),
                "Failed to get Application AAssetManager"
            );
            let app_asset_manager =
                AssetManager::from_ptr(std::ptr::NonNull::new(asset_manager_ptr).unwrap());

            let main_callbacks = MainCallbacks::new(java_main_looper);

            AppState {
                main_callbacks,
                app_asset_manager,
            }
        });

        if let Err(err) = try_init_current_thread(env, jni_activity) {
            eprintln!("Failed to initialize Java thread state: {:?}", err);
        }

        let asset_manager = unsafe { AssetManager::from_ptr(app_state.app_asset_manager.ptr()) };
        let main_callbacks = app_state.main_callbacks.clone();

        Ok((asset_manager, main_callbacks))
    })
}
