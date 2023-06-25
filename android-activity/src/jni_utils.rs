//! The JNI calls we make in this crate are often not part of a Java native
//! method implementation and so we can't assume we have a JNI local frame that
//! is going to unwind and free local references, and we also can't just leave
//! exceptions to get thrown when returning to Java.
//!
//! These utilities help us check + clear exceptions and map them into Rust Errors.

use std::{ops::Deref, sync::Arc};

use jni::{
    objects::{JObject, JString},
    JavaVM,
};

use crate::{
    error::{InternalAppError, InternalResult},
    input::{KeyCharacterMap, KeyCharacterMapBinding},
};

// TODO: JavaVM should implement Clone
#[derive(Debug)]
pub(crate) struct CloneJavaVM {
    pub jvm: JavaVM,
}
impl Clone for CloneJavaVM {
    fn clone(&self) -> Self {
        Self {
            jvm: unsafe { JavaVM::from_raw(self.jvm.get_java_vm_pointer()).unwrap() },
        }
    }
}
impl CloneJavaVM {
    pub unsafe fn from_raw(jvm: *mut jni_sys::JavaVM) -> InternalResult<Self> {
        Ok(Self {
            jvm: JavaVM::from_raw(jvm)?,
        })
    }
}
unsafe impl Send for CloneJavaVM {}
unsafe impl Sync for CloneJavaVM {}

impl Deref for CloneJavaVM {
    type Target = JavaVM;

    fn deref(&self) -> &Self::Target {
        &self.jvm
    }
}

/// Use with `.map_err()` to map `jni::errors::Error::JavaException` into a
/// richer error based on the actual contents of the `JThrowable`
///
/// (The `jni` crate doesn't do that automatically since it's more
/// common to let the exception get thrown when returning to Java)
///
/// This will also clear the exception
pub(crate) fn clear_and_map_exception_to_err(
    env: &mut jni::JNIEnv<'_>,
    err: jni::errors::Error,
) -> InternalAppError {
    if matches!(err, jni::errors::Error::JavaException) {
        let result = env.with_local_frame::<_, _, InternalAppError>(5, |env| {
            let e = env.exception_occurred()?;
            assert!(!e.is_null()); // should only be called after receiving a JavaException Result
            env.exception_clear()?;

            let class = env.get_object_class(&e)?;
            //let get_stack_trace_method = env.get_method_id(&class, "getStackTrace", "()[Ljava/lang/StackTraceElement;")?;
            let get_message_method =
                env.get_method_id(&class, "getMessage", "()Ljava/lang/String;")?;

            let msg = unsafe {
                env.call_method_unchecked(
                    &e,
                    get_message_method,
                    jni::signature::ReturnType::Object,
                    &[],
                )?
                .l()
                .unwrap()
            };
            let msg = unsafe { JString::from_raw(JObject::into_raw(msg)) };
            let msg = env.get_string(&msg)?;
            let msg: String = msg.into();

            // TODO: get Java backtrace:
            /*
            if let JValue::Object(elements) = env.call_method_unchecked(&e, get_stack_trace_method, jni::signature::ReturnType::Array, &[])? {
                let elements = env.auto_local(elements);

            }
            */

            Ok(msg)
        });

        match result {
            Ok(msg) => InternalAppError::JniException(msg),
            Err(err) => InternalAppError::JniException(format!(
                "UNKNOWN (Failed to query JThrowable: {err:?})"
            )),
        }
    } else {
        err.into()
    }
}

pub(crate) fn device_key_character_map(
    jvm: CloneJavaVM,
    key_map_binding: Arc<KeyCharacterMapBinding>,
    device_id: i32,
) -> InternalResult<KeyCharacterMap> {
    // Don't really need to 'attach' since this should be called from the app's main thread that
    // should already be attached, but the redundancy should be fine
    //
    // Attach 'permanently' to avoid any chance of detaching the thread from the VM
    let mut env = jvm.attach_current_thread_permanently()?;

    // We don't want to accidentally leak any local references while we
    // aren't going to be returning from here back to the JVM, to unwind, so
    // we make a local frame
    let character_map = env.with_local_frame::<_, _, jni::errors::Error>(10, |env| {
        let input_device_class = env.find_class("android/view/InputDevice")?; // Creates a local ref
        let device = env
            .call_static_method(
                input_device_class,
                "getDevice",
                "(I)Landroid/view/InputDevice;",
                &[device_id.into()],
            )?
            .l()?; // Creates a local ref

        let character_map = env
            .call_method(
                &device,
                "getKeyCharacterMap",
                "()Landroid/view/KeyCharacterMap;",
                &[],
            )?
            .l()?;
        let character_map = env.new_global_ref(character_map)?;

        Ok(character_map)
    })?;

    Ok(KeyCharacterMap::new(
        jvm.clone(),
        key_map_binding,
        character_map,
    ))
}
