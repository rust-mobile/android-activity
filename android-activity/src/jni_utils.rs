//! The JNI calls we make in this crate are often not part of a Java native
//! method implementation and so we can't assume we have a JNI local frame that
//! is going to unwind and free local references, and we also can't just leave
//! exceptions to get thrown when returning to Java.
//!
//! These utilities help us check + clear exceptions and map them into Rust Errors.

use crate::error::InternalAppError;

fn try_get_stack_trace(
    env: &mut jni::Env<'_>,
    throwable: &jni::objects::JThrowable,
) -> jni::errors::Result<String> {
    let stack_trace = throwable.get_stack_trace(env)?;
    let len = stack_trace.len(env)?;
    let mut trace = String::new();
    for i in 0..len {
        let element = stack_trace.get_element(env, i)?;
        let element_jstr = element.try_to_string(env)?;
        trace.push_str(&format!("{i}: {element_jstr}\n"));
    }
    Ok(trace)
}

/// Use with `.map_err()` to map `jni::errors::Error::JavaException` into a
/// richer error based on the actual contents of the `JThrowable`
///
/// (The `jni` crate doesn't do that automatically since it's more
/// common to let the exception get thrown when returning to Java)
///
/// This will also clear the exception
pub(crate) fn clear_and_map_exception_to_err(
    env: &mut jni::Env<'_>,
    err: jni::errors::Error,
) -> InternalAppError {
    if matches!(err, jni::errors::Error::JavaException) {
        let result = env.with_local_frame::<_, _, InternalAppError>(5, |env| {
            let Some(e) = env.exception_occurred() else {
                // should only be called after receiving a JavaException Result
                unreachable!("JNI Error was JavaException but no exception was set");
            };
            env.exception_clear();

            let msg = e.get_message(env)?;
            let mut msg: String = msg.to_string();
            match try_get_stack_trace(env, &e) {
                Ok(stack_trace) => {
                    msg.push_str("stack trace:\n");
                    msg.push_str(&stack_trace);
                }
                Err(err) => {
                    msg.push_str(&format!("\nfailed to get stack trace: {err:?}"));
                }
            }

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
