use jni::{
    errors::Error,
    objects::{JObject, JString},
    JNIEnv,
};

mod action;
pub use action::Action;

mod extra;
pub use extra::Extra;

struct Inner<'env> {
    env: JNIEnv<'env>,
    object: JObject<'env>,
}

/// A messaging object you can use to request an action from another android app component.
#[must_use]
pub struct Intent<'env> {
    inner: Result<Inner<'env>, Error>,
}

impl<'env> Intent<'env> {
    pub fn from_object(env: JNIEnv<'env>, object: JObject<'env>) -> Self {
        Self {
            inner: Ok(Inner { env, object }),
        }
    }

    fn from_fn(f: impl FnOnce() -> Result<Inner<'env>, Error>) -> Self {
        let inner = f();
        Self { inner }
    }

    pub fn new(env: JNIEnv<'env>, action: impl AsRef<str>) -> Self {
        Self::from_fn(|| {
            let intent_class = env.find_class("android/content/Intent")?;
            let action_view =
                env.get_static_field(intent_class, action.as_ref(), "Ljava/lang/String;")?;

            let intent =
                env.new_object(intent_class, "(Ljava/lang/String;)V", &[action_view.into()])?;

            Ok(Inner {
                env,
                object: intent,
            })
        })
    }

    pub fn new_with_uri(env: JNIEnv<'env>, action: impl AsRef<str>, uri: impl AsRef<str>) -> Self {
        Self::from_fn(|| {
            let url_string = env.new_string(uri)?;
            let uri_class = env.find_class("android/net/Uri")?;
            let uri = env.call_static_method(
                uri_class,
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[JString::from(url_string).into()],
            )?;

            let intent_class = env.find_class("android/content/Intent")?;
            let action_view =
                env.get_static_field(intent_class, action.as_ref(), "Ljava/lang/String;")?;

            let intent = env.new_object(
                intent_class,
                "(Ljava/lang/String;Landroid/net/Uri;)V",
                &[action_view.into(), uri.into()],
            )?;

            Ok(Inner {
                env,
                object: intent,
            })
        })
    }

    /// Add extended data to the intent.
    /// ```no_run
    /// use android_intent::{Action, Extra, Intent};
    ///
    /// # android_intent::with_current_env(|env| {
    /// let intent = Intent::new(env, Action::Send);
    /// intent.push_extra(Extra::Text, "Hello World!")
    /// # })
    /// ```
    pub fn with_extra(self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.and_then(|inner| {
            let key = inner.env.new_string(key)?;
            let value = inner.env.new_string(value)?;

            inner.env.call_method(
                inner.object,
                "putExtra",
                "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
                &[key.into(), value.into()],
            )?;

            Ok(inner)
        })
    }

    /// Builds a new [`Action::Chooser`](Action) Intent that wraps the given target intent.
    /// ```no_run
    /// use android_intent::{Action, Intent};
    ///
    /// # android_intent::with_current_env(|env| {
    /// let intent = Intent::new(env, Action::Send).into_chhoser();
    /// # })
    /// ```
    pub fn into_chooser(self) -> Self {
        self.into_chooser_with_title(None::<&str>)
    }

    pub fn into_chooser_with_title(self, title: Option<impl AsRef<str>>) -> Self {
        self.and_then(|mut inner| {
            let title_value = if let Some(title) = title {
                let s = inner.env.new_string(title)?;
                s.into()
            } else {
                JObject::null().into()
            };

            let intent_class = inner.env.find_class("android/content/Intent")?;
            let intent = inner.env.call_static_method(
                intent_class,
                "createChooser",
                "(Landroid/content/Intent;Ljava/lang/CharSequence;)Landroid/content/Intent;",
                &[inner.object.into(), title_value],
            )?;

            inner.object = intent.try_into()?;
            Ok(inner)
        })
    }

    /// Set an explicit MIME data type.
    /// ```no_run
    /// use android_intent::{Action, Intent};
    ///
    /// # android_intent::with_current_env(|env| {
    /// let intent = Intent::new(env, Action::Send);
    /// intent.set_type("text/plain");
    /// # })
    /// ```
    pub fn with_type(self, type_name: impl AsRef<str>) -> Self {
        self.and_then(|inner| {
            let jstring = inner.env.new_string(type_name)?;

            inner.env.call_method(
                inner.object,
                "setType",
                "(Ljava/lang/String;)Landroid/content/Intent;",
                &[jstring.into()],
            )?;

            Ok(inner)
        })
    }

    pub fn start_activity(self) -> Result<(), Error> {
        let cx = ndk_context::android_context();
        let activity = unsafe { JObject::from_raw(cx.context() as jni::sys::jobject) };

        self.inner.and_then(|inner| {
            inner.env.call_method(
                activity,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[inner.object.into()],
            )?;

            Ok(())
        })
    }

    fn and_then(mut self, f: impl FnOnce(Inner) -> Result<Inner, Error>) -> Self {
        self.inner = self.inner.and_then(f);
        self
    }
}
