/// Extra data to include with an intent
pub enum Extra {
    Text,
}

impl AsRef<str> for Extra {
    fn as_ref(&self) -> &str {
        match self {
            Self::Text => "android.intent.extra.TEXT",
        }
    }
}
