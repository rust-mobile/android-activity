use std::sync::Arc;

use jni::{
    objects::{GlobalRef, JClass, JMethodID, JObject, JStaticMethodID, JValue},
    signature::{Primitive, ReturnType},
    JNIEnv,
};
use jni_sys::jint;

use crate::{
    input::{Keycode, MetaState},
    jni_utils::CloneJavaVM,
};

use crate::{
    error::{AppError, InternalAppError},
    jni_utils,
};

/// An enum representing the types of keyboards that may generate key events
///
/// See [getKeyboardType() docs](https://developer.android.com/reference/android/view/KeyCharacterMap#getKeyboardType())
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// This implements `Into<u32>` and `From<u32>` for converting to/from Android
/// SDK integer values.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, num_enum::FromPrimitive, num_enum::IntoPrimitive,
)]
#[non_exhaustive]
#[repr(u32)]
pub enum KeyboardType {
    /// A numeric (12-key) keyboard.
    ///
    /// A numeric keyboard supports text entry using a multi-tap approach. It may be necessary to tap a key multiple times to generate the desired letter or symbol.
    ///
    /// This type of keyboard is generally designed for thumb typing.
    Numeric,

    /// A keyboard with all the letters, but with more than one letter per key.
    ///
    /// This type of keyboard is generally designed for thumb typing.
    Predictive,

    /// A keyboard with all the letters, and maybe some numbers.
    ///
    /// An alphabetic keyboard supports text entry directly but may have a condensed layout with a small form factor. In contrast to a full keyboard, some symbols may only be accessible using special on-screen character pickers. In addition, to improve typing speed and accuracy, the framework provides special affordances for alphabetic keyboards such as auto-capitalization and toggled / locked shift and alt keys.
    ///
    /// This type of keyboard is generally designed for thumb typing.
    Alpha,

    /// A full PC-style keyboard.
    ///
    /// A full keyboard behaves like a PC keyboard. All symbols are accessed directly by pressing keys on the keyboard without on-screen support or affordances such as auto-capitalization.
    ///
    /// This type of keyboard is generally designed for full two hand typing.
    Full,

    /// A keyboard that is only used to control special functions rather than for typing.
    ///
    /// A special function keyboard consists only of non-printing keys such as HOME and POWER that are not actually used for typing.
    SpecialFunction,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// Either represents, a unicode character or combining accent from a
/// [`KeyCharacterMap`], or `None` for non-printable keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyMapChar {
    None,
    Unicode(char),
    CombiningAccent(char),
}

// I've also tried to think here about how to we could potentially automatically
// generate a binding struct like `KeyCharacterMapBinding` with a procmacro and
// so have intentionally limited the `Binding` being a very thin, un-opinionated
// wrapper based on basic JNI types.

/// Lower-level JNI binding for `KeyCharacterMap` class only holds 'static state
/// and can be shared with an `Arc` ref count.
///
/// The separation here also neatly helps us separate `InternalAppError` from
/// `AppError` for mapping JNI errors without exposing any `jni-rs` types in the
/// public API.
#[derive(Debug)]
pub(crate) struct KeyCharacterMapBinding {
    //vm: JavaVM,
    klass: GlobalRef,
    get_method_id: JMethodID,
    get_dead_char_method_id: JStaticMethodID,
    get_keyboard_type_method_id: JMethodID,
}

impl KeyCharacterMapBinding {
    pub(crate) fn new(env: &mut JNIEnv) -> Result<Self, InternalAppError> {
        let binding = env.with_local_frame::<_, _, InternalAppError>(10, |env| {
            let klass = env.find_class("android/view/KeyCharacterMap")?; // Creates a local ref
            Ok(Self {
                get_method_id: env.get_method_id(&klass, "get", "(II)I")?,
                get_dead_char_method_id: env.get_static_method_id(
                    &klass,
                    "getDeadChar",
                    "(II)I",
                )?,
                get_keyboard_type_method_id: env.get_method_id(&klass, "getKeyboardType", "()I")?,
                klass: env.new_global_ref(&klass)?,
            })
        })?;
        Ok(binding)
    }

    pub fn get<'local>(
        &self,
        env: &'local mut JNIEnv,
        key_map: impl AsRef<JObject<'local>>,
        key_code: jint,
        meta_state: jint,
    ) -> Result<jint, InternalAppError> {
        let key_map = key_map.as_ref();

        // Safety:
        // - we know our global `key_map` reference is non-null and valid.
        // - we know `get_method_id` remains valid
        // - we know that the signature of KeyCharacterMap::get is `(int, int) -> int`
        // - we know this won't leak any local references as a side effect
        //
        // We know it's ok to unwrap the `.i()` value since we explicitly
        // specify the return type as `Int`
        let unicode = unsafe {
            env.call_method_unchecked(
                key_map,
                self.get_method_id,
                ReturnType::Primitive(Primitive::Int),
                &[
                    JValue::Int(key_code).as_jni(),
                    JValue::Int(meta_state).as_jni(),
                ],
            )
        }
        .map_err(|err| jni_utils::clear_and_map_exception_to_err(env, err))?;
        Ok(unicode.i().unwrap())
    }

    pub fn get_dead_char(
        &self,
        env: &mut JNIEnv,
        accent_char: jint,
        base_char: jint,
    ) -> Result<jint, InternalAppError> {
        // Safety:
        // - we know `get_dead_char_method_id` remains valid
        // - we know that KeyCharacterMap::getDeadKey is a static method
        // - we know that the signature of KeyCharacterMap::getDeadKey is `(int, int) -> int`
        // - we know this won't leak any local references as a side effect
        //
        // We know it's ok to unwrap the `.i()` value since we explicitly
        // specify the return type as `Int`

        // Urgh, it's pretty terrible that there's no ergonomic/safe way to get a JClass reference from a GlobalRef
        // Safety: we don't do anything that would try to delete the JClass as if it were a real local reference
        let klass = unsafe { JClass::from_raw(self.klass.as_obj().as_raw()) };
        let unicode = unsafe {
            env.call_static_method_unchecked(
                &klass,
                self.get_dead_char_method_id,
                ReturnType::Primitive(Primitive::Int),
                &[
                    JValue::Int(accent_char).as_jni(),
                    JValue::Int(base_char).as_jni(),
                ],
            )
        }
        .map_err(|err| jni_utils::clear_and_map_exception_to_err(env, err))?;
        Ok(unicode.i().unwrap())
    }

    pub fn get_keyboard_type<'local>(
        &self,
        env: &'local mut JNIEnv,
        key_map: impl AsRef<JObject<'local>>,
    ) -> Result<jint, InternalAppError> {
        let key_map = key_map.as_ref();

        // Safety:
        // - we know our global `key_map` reference is non-null and valid.
        // - we know `get_keyboard_type_method_id` remains valid
        // - we know that the signature of KeyCharacterMap::getKeyboardType is `() -> int`
        // - we know this won't leak any local references as a side effect
        //
        // We know it's ok to unwrap the `.i()` value since we explicitly
        // specify the return type as `Int`
        Ok(unsafe {
            env.call_method_unchecked(
                key_map,
                self.get_keyboard_type_method_id,
                ReturnType::Primitive(Primitive::Int),
                &[],
            )
        }
        .map_err(|err| jni_utils::clear_and_map_exception_to_err(env, err))?
        .i()
        .unwrap())
    }
}

/// Describes the keys provided by a keyboard device and their associated labels.
#[derive(Clone, Debug)]
pub struct KeyCharacterMap {
    jvm: CloneJavaVM,
    binding: Arc<KeyCharacterMapBinding>,
    key_map: GlobalRef,
}

impl KeyCharacterMap {
    pub(crate) fn new(
        jvm: CloneJavaVM,
        binding: Arc<KeyCharacterMapBinding>,
        key_map: GlobalRef,
    ) -> Self {
        Self {
            jvm,
            binding,
            key_map,
        }
    }

    /// Gets the Unicode character generated by the specified [`Keycode`] and [`MetaState`] combination.
    ///
    /// Returns [`KeyMapChar::None`] if the key is not one that is used to type Unicode characters.
    ///
    /// Returns [`KeyMapChar::CombiningAccent`] if the key is a "dead key" that should be combined with
    /// another to actually produce a character -- see [`KeyCharacterMap::get_dead_char`].
    ///
    /// # Errors
    ///
    /// Since this API needs to use JNI internally to call into the Android JVM it may return
    /// a [`AppError::JavaError`] in case there is a spurious JNI error or an exception
    /// is caught.
    pub fn get(&self, key_code: Keycode, meta_state: MetaState) -> Result<KeyMapChar, AppError> {
        let key_code: u32 = key_code.into();
        let key_code = key_code as jni_sys::jint;
        let meta_state: u32 = meta_state.0;
        let meta_state = meta_state as jni_sys::jint;

        // Since we expect this API to be called from the `main` thread then we expect to already be
        // attached to the JVM
        //
        // Safety: there's no other JNIEnv in scope so this env can't be used to subvert the mutable
        // borrow rules that ensure we can only add local references to the top JNI frame.
        let mut env = self.jvm.get_env().map_err(|err| {
            let err: InternalAppError = err.into();
            err
        })?;
        let unicode = self
            .binding
            .get(&mut env, self.key_map.as_obj(), key_code, meta_state)?;
        let unicode = unicode as u32;

        const COMBINING_ACCENT: u32 = 0x80000000;
        const COMBINING_ACCENT_MASK: u32 = !COMBINING_ACCENT;

        if unicode == 0 {
            Ok(KeyMapChar::None)
        } else if unicode & COMBINING_ACCENT == COMBINING_ACCENT {
            let accent = unicode & COMBINING_ACCENT_MASK;
            // Safety: assumes Android key maps don't contain invalid unicode characters
            Ok(KeyMapChar::CombiningAccent(unsafe {
                char::from_u32_unchecked(accent)
            }))
        } else {
            // Safety: assumes Android key maps don't contain invalid unicode characters
            Ok(KeyMapChar::Unicode(unsafe {
                char::from_u32_unchecked(unicode)
            }))
        }
    }

    /// Get the character that is produced by combining the dead key producing accent with the key producing character c.
    ///
    /// For example, ```get_dead_char('`', 'e')``` returns 'è'. `get_dead_char('^', ' ')` returns '^' and `get_dead_char('^', '^')` returns '^'.
    ///
    /// # Errors
    ///
    /// Since this API needs to use JNI internally to call into the Android JVM it may return
    /// a [`AppError::JavaError`] in case there is a spurious JNI error or an exception
    /// is caught.
    pub fn get_dead_char(
        &self,
        accent_char: char,
        base_char: char,
    ) -> Result<Option<char>, AppError> {
        let accent_char = accent_char as jni_sys::jint;
        let base_char = base_char as jni_sys::jint;

        // Since we expect this API to be called from the `main` thread then we expect to already be
        // attached to the JVM
        //
        // Safety: there's no other JNIEnv in scope so this env can't be used to subvert the mutable
        // borrow rules that ensure we can only add local references to the top JNI frame.
        let mut env = self.jvm.get_env().map_err(|err| {
            let err: InternalAppError = err.into();
            err
        })?;
        let unicode = self
            .binding
            .get_dead_char(&mut env, accent_char, base_char)?;
        let unicode = unicode as u32;

        // Safety: assumes Android key maps don't contain invalid unicode characters
        Ok(if unicode == 0 {
            None
        } else {
            Some(unsafe { char::from_u32_unchecked(unicode) })
        })
    }

    /// Gets the keyboard type.
    ///
    /// Different keyboard types have different semantics. See [`KeyboardType`] for details.
    ///
    /// # Errors
    ///
    /// Since this API needs to use JNI internally to call into the Android JVM it may return
    /// a [`AppError::JavaError`] in case there is a spurious JNI error or an exception
    /// is caught.
    pub fn get_keyboard_type(&self) -> Result<KeyboardType, AppError> {
        // Since we expect this API to be called from the `main` thread then we expect to already be
        // attached to the JVM
        //
        // Safety: there's no other JNIEnv in scope so this env can't be used to subvert the mutable
        // borrow rules that ensure we can only add local references to the top JNI frame.
        let mut env = self.jvm.get_env().map_err(|err| {
            let err: InternalAppError = err.into();
            err
        })?;
        let keyboard_type = self
            .binding
            .get_keyboard_type(&mut env, self.key_map.as_obj())?;
        let keyboard_type = keyboard_type as u32;
        Ok(keyboard_type.into())
    }
}
