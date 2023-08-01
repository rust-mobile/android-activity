use bitflags::bitflags;
use num_enum::{IntoPrimitive, TryFromPrimitive};

pub use crate::activity_impl::input::*;

/// An enum representing the source of an [`MotionEvent`] or [`KeyEvent`]
///
/// See [the InputDevice docs](https://developer.android.com/reference/android/view/InputDevice#SOURCE_ANY)
#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum Source {
    BluetoothStylus = 0x0000c002,
    Dpad = 0x00000201,
    /// Either a gamepad or a joystick
    Gamepad = 0x00000401,
    Hdmi = 0x02000001,
    /// Either a gamepad or a joystick
    Joystick = 0x01000010,
    /// Pretty much any device with buttons. Query the keyboard type to determine
    /// if it has alphabetic keys and can be used for text entry.
    Keyboard = 0x00000101,
    /// A pointing device, such as a mouse or trackpad
    Mouse = 0x00002002,
    /// A pointing device, such as a mouse or trackpad whose relative motions should be treated as navigation events
    MouseRelative = 0x00020004,
    /// An input device akin to a scroll wheel
    RotaryEncoder = 0x00400000,
    Sensor = 0x04000000,
    Stylus = 0x00004002,
    Touchpad = 0x00100008,
    Touchscreen = 0x00001002,
    TouchNavigation = 0x00200000,
    Trackball = 0x00010004,

    Unknown = 0,
}

bitflags! {
    struct SourceFlags: u32 {
        const CLASS_MASK = 0x000000ff;

        const BUTTON = 0x00000001;
        const POINTER = 0x00000002;
        const TRACKBALL = 0x00000004;
        const POSITION = 0x00000008;
        const JOYSTICK = 0x00000010;
        const NONE = 0;
    }
}

/// An enum representing the class of a [`MotionEvent`] or [`KeyEvent`] source
///
/// See [the InputDevice docs](https://developer.android.com/reference/android/view/InputDevice#SOURCE_CLASS_MASK)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    None,
    Button,
    Pointer,
    Trackball,
    Position,
    Joystick,
}

impl From<u32> for Class {
    fn from(source: u32) -> Self {
        let class = SourceFlags::from_bits_truncate(source);
        match class {
            SourceFlags::BUTTON => Class::Button,
            SourceFlags::POINTER => Class::Pointer,
            SourceFlags::TRACKBALL => Class::Trackball,
            SourceFlags::POSITION => Class::Position,
            SourceFlags::JOYSTICK => Class::Joystick,
            _ => Class::None,
        }
    }
}

impl From<Source> for Class {
    fn from(source: Source) -> Self {
        let source: u32 = source.into();
        source.into()
    }
}

/// This struct holds a span within a region of text from `start` to `end`.
///
/// The `start` index may be greater than the `end` index (swapping `start` and `end` will represent the same span)
///
/// The lower index is inclusive and the higher index is exclusive.
///
/// An empty span or cursor position is specified with `start == end`.
///
#[derive(Debug, Clone, Copy)]
pub struct TextSpan {
    /// The start of the span (inclusive)
    pub start: usize,

    /// The end of the span (exclusive)
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct TextInputState {
    pub text: String,

    /// A selection defined on the text.
    ///
    /// To set the cursor position, start and end should have the same value.
    ///
    /// Changing the selection has no effect on the compose_region.
    pub selection: TextSpan,

    /// A composing region defined on the text.
    ///
    /// When being set, then if there was a composing region, the region is replaced.
    ///
    /// The given indices will be clamped to the `text` bounds
    ///
    /// If the resulting region is zero-sized, no region is marked (equivalent to passing `None`)
    pub compose_region: Option<TextSpan>,
}
