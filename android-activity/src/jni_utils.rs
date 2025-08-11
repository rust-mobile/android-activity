//! The JNI calls we make in this crate are often not part of a Java native
//! method implementation and so we can't assume we have a JNI local frame that
//! is going to unwind and free local references, and we also can't just leave
//! exceptions to get thrown when returning to Java.
//!
//! These utilities help us check + clear exceptions and map them into Rust Errors.

use std::sync::Arc;

use jni::{
    objects::{JObject, JString},
    JNIVersion, JavaVM,
};

use crate::{
    error::{InternalAppError, InternalResult},
    input::{KeyCharacterMap, KeyCharacterMapBinding},
};

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
            let Some(e) = env.exception_occurred() else {
                // should only be called after receiving a JavaException Result
                unreachable!("JNI Error was JavaException but no exception was set");
            };
            env.exception_clear();

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

pub(crate) fn device_key_character_map_with_env(
    env: &mut jni::JNIEnv<'_>,
    key_map_binding: Arc<KeyCharacterMapBinding>,
    device_id: i32,
) -> jni::errors::Result<KeyCharacterMap> {
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

    Ok(KeyCharacterMap::new(
        env.get_java_vm().clone(),
        key_map_binding,
        character_map,
    ))
}

pub(crate) fn device_key_character_map(
    jvm: JavaVM,
    key_map_binding: Arc<KeyCharacterMapBinding>,
    device_id: i32,
) -> InternalResult<KeyCharacterMap> {
    jvm.attach_current_thread(JNIVersion::V1_4, |env| {
        device_key_character_map_with_env(env, key_map_binding, device_id)
            .map_err(|err| clear_and_map_exception_to_err(env, err))
    })
}
