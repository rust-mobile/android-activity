pub use ndk::event::{
    Axis, EdgeFlags, KeyAction, KeyEventFlags, Keycode, MetaState, MotionAction, MotionEventFlags,
    Source, SourceClass, ToolType,
};

pub use crate::activity_impl::input::*;
use crate::InputStatus;

mod sdk;
pub use sdk::*;

/// Identifies buttons that are associated with motion events.
///
/// See [the NDK
/// docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-47)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// This implements `Into<u32>` and `From<u32>` for converting to/from Android
/// SDK integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum Button {
    Back = ndk_sys::AMOTION_EVENT_BUTTON_BACK,
    Forward = ndk_sys::AMOTION_EVENT_BUTTON_FORWARD,
    Primary = ndk_sys::AMOTION_EVENT_BUTTON_PRIMARY,
    Secondary = ndk_sys::AMOTION_EVENT_BUTTON_SECONDARY,
    StylusPrimary = ndk_sys::AMOTION_EVENT_BUTTON_STYLUS_PRIMARY,
    StylusSecondary = ndk_sys::AMOTION_EVENT_BUTTON_STYLUS_SECONDARY,
    Tertiary = ndk_sys::AMOTION_EVENT_BUTTON_TERTIARY,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
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

// Represents the action button on a soft keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(i32)]
pub enum TextInputAction {
    /// Let receiver decide what logical action to perform
    Unspecified = 0,
    /// No action - receiver could instead interpret as an "enter" key that inserts a newline character
    None = 1,
    /// Navigate to the input location (such as a URL)
    Go = 2,
    /// Search based on the input text
    Search = 3,
    /// Send the input to the target
    Send = 4,
    /// Move to the next input field
    Next = 5,
    /// Indicate that input is done
    Done = 6,
    /// Move to the previous input field
    Previous = 7,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(i32),
}

/// An exclusive, lending iterator for input events
pub struct InputIterator<'a> {
    pub(crate) inner: crate::activity_impl::InputIteratorInner<'a>,
}

impl InputIterator<'_> {
    /// Reads and handles the next input event by passing it to the given `callback`
    ///
    /// `callback` should return [`InputStatus::Unhandled`] for any input events that aren't directly
    /// handled by the application, or else [`InputStatus::Handled`]. Unhandled events may lead to a
    /// fallback interpretation of the event.
    pub fn next<F>(&mut self, callback: F) -> bool
    where
        F: FnOnce(&crate::activity_impl::input::InputEvent) -> InputStatus,
    {
        self.inner.next(callback)
    }
}

/// A view into the data of a specific pointer in a motion event.
#[derive(Debug)]
pub struct Pointer<'a> {
    pub(crate) inner: PointerImpl<'a>,
}

impl Pointer<'_> {
    #[inline]
    pub fn pointer_index(&self) -> usize {
        self.inner.pointer_index()
    }

    #[inline]
    pub fn pointer_id(&self) -> i32 {
        self.inner.pointer_id()
    }

    #[inline]
    pub fn axis_value(&self, axis: Axis) -> f32 {
        self.inner.axis_value(axis)
    }

    #[inline]
    pub fn orientation(&self) -> f32 {
        self.axis_value(Axis::Orientation)
    }

    #[inline]
    pub fn pressure(&self) -> f32 {
        self.axis_value(Axis::Pressure)
    }

    #[inline]
    pub fn raw_x(&self) -> f32 {
        self.inner.raw_x()
    }

    #[inline]
    pub fn raw_y(&self) -> f32 {
        self.inner.raw_y()
    }

    #[inline]
    pub fn x(&self) -> f32 {
        self.axis_value(Axis::X)
    }

    #[inline]
    pub fn y(&self) -> f32 {
        self.axis_value(Axis::Y)
    }

    #[inline]
    pub fn size(&self) -> f32 {
        self.axis_value(Axis::Size)
    }

    #[inline]
    pub fn tool_major(&self) -> f32 {
        self.axis_value(Axis::ToolMajor)
    }

    #[inline]
    pub fn tool_minor(&self) -> f32 {
        self.axis_value(Axis::ToolMinor)
    }

    #[inline]
    pub fn touch_major(&self) -> f32 {
        self.axis_value(Axis::TouchMajor)
    }

    #[inline]
    pub fn touch_minor(&self) -> f32 {
        self.axis_value(Axis::TouchMinor)
    }

    #[inline]
    pub fn tool_type(&self) -> ToolType {
        self.inner.tool_type()
    }
}

/// An iterator over the pointers in a [`MotionEvent`].
#[derive(Debug)]
pub struct PointersIter<'a> {
    pub(crate) inner: PointersIterImpl<'a>,
}

impl<'a> Iterator for PointersIter<'a> {
    type Item = Pointer<'a>;
    fn next(&mut self) -> Option<Pointer<'a>> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for PointersIter<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}
