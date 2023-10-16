use bitflags::bitflags;

pub use crate::activity_impl::input::*;
use crate::InputStatus;

mod sdk;
pub use sdk::*;

/// An enum representing the source of an [`MotionEvent`] or [`KeyEvent`]
///
/// See [the InputDevice docs](https://developer.android.com/reference/android/view/InputDevice#SOURCE_ANY)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
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

    // We need to consider that the enum variants may be extended across
    // different versions of Android (i.e. effectively at runtime) but at the
    // same time we don't want it to be an API break to extend this enum in
    // future releases of `android-activity` with new variants from the latest
    // NDK/SDK.
    //
    // We can't just use `#[non_exhaustive]` because that only really helps
    // when adding new variants in sync with android-activity releases.
    //
    // On the other hand we also can't rely on a catch-all `Unknown(u32)` that
    // only really helps with unknown variants seen at runtime.
    //
    // What we aim for instead is to have a hidden catch-all variant that
    // is considered (practically) unmatchable so code is forced to have
    // a `unknown => {}` catch-all pattern match that will cover unknown variants
    // either in the form of Rust variants added in future versions or
    // in the form of an `__Unknown(u32)` integer that represents an unknown
    // variant seen at runtime.
    //
    // Any `unknown => {}` pattern match can rely on `IntoPrimitive` to convert
    // the `unknown` variant to the integer that comes from the Android SDK
    // in case that values needs to be passed on, even without knowing its
    // semantic meaning at compile time.
    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

// ndk_sys doesn't currently have the `TRACKBALL` flag so we define our
// own internal class constants for now
bitflags! {
    #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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

impl Source {
    #[inline]
    pub fn is_button_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::BUTTON)
    }
    #[inline]
    pub fn is_pointer_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::POINTER)
    }
    #[inline]
    pub fn is_trackball_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::TRACKBALL)
    }
    #[inline]
    pub fn is_position_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::POSITION)
    }
    #[inline]
    pub fn is_joystick_class(self) -> bool {
        let class = SourceFlags::from_bits_truncate(self.into());
        class.contains(SourceFlags::JOYSTICK)
    }
}

/// A bitfield representing the state of modifier keys during an event.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-25)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MetaState(pub u32);

impl MetaState {
    #[inline]
    pub fn alt_on(self) -> bool {
        self.0 & ndk_sys::AMETA_ALT_ON != 0
    }
    #[inline]
    pub fn alt_left_on(self) -> bool {
        self.0 & ndk_sys::AMETA_ALT_LEFT_ON != 0
    }
    #[inline]
    pub fn alt_right_on(self) -> bool {
        self.0 & ndk_sys::AMETA_ALT_RIGHT_ON != 0
    }
    #[inline]
    pub fn shift_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SHIFT_ON != 0
    }
    #[inline]
    pub fn shift_left_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SHIFT_LEFT_ON != 0
    }
    #[inline]
    pub fn shift_right_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SHIFT_RIGHT_ON != 0
    }
    #[inline]
    pub fn sym_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SYM_ON != 0
    }
    #[inline]
    pub fn function_on(self) -> bool {
        self.0 & ndk_sys::AMETA_FUNCTION_ON != 0
    }
    #[inline]
    pub fn ctrl_on(self) -> bool {
        self.0 & ndk_sys::AMETA_CTRL_ON != 0
    }
    #[inline]
    pub fn ctrl_left_on(self) -> bool {
        self.0 & ndk_sys::AMETA_CTRL_LEFT_ON != 0
    }
    #[inline]
    pub fn ctrl_right_on(self) -> bool {
        self.0 & ndk_sys::AMETA_CTRL_RIGHT_ON != 0
    }
    #[inline]
    pub fn meta_on(self) -> bool {
        self.0 & ndk_sys::AMETA_META_ON != 0
    }
    #[inline]
    pub fn meta_left_on(self) -> bool {
        self.0 & ndk_sys::AMETA_META_LEFT_ON != 0
    }
    #[inline]
    pub fn meta_right_on(self) -> bool {
        self.0 & ndk_sys::AMETA_META_RIGHT_ON != 0
    }
    #[inline]
    pub fn caps_lock_on(self) -> bool {
        self.0 & ndk_sys::AMETA_CAPS_LOCK_ON != 0
    }
    #[inline]
    pub fn num_lock_on(self) -> bool {
        self.0 & ndk_sys::AMETA_NUM_LOCK_ON != 0
    }
    #[inline]
    pub fn scroll_lock_on(self) -> bool {
        self.0 & ndk_sys::AMETA_SCROLL_LOCK_ON != 0
    }
}

impl From<ndk::event::MetaState> for MetaState {
    fn from(value: ndk::event::MetaState) -> Self {
        Self(value.0)
    }
}

/// A motion action.
///
/// See [the NDK
/// docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-29)
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
pub enum MotionAction {
    Down = ndk_sys::AMOTION_EVENT_ACTION_DOWN,
    Up = ndk_sys::AMOTION_EVENT_ACTION_UP,
    Move = ndk_sys::AMOTION_EVENT_ACTION_MOVE,
    Cancel = ndk_sys::AMOTION_EVENT_ACTION_CANCEL,
    Outside = ndk_sys::AMOTION_EVENT_ACTION_OUTSIDE,
    PointerDown = ndk_sys::AMOTION_EVENT_ACTION_POINTER_DOWN,
    PointerUp = ndk_sys::AMOTION_EVENT_ACTION_POINTER_UP,
    HoverMove = ndk_sys::AMOTION_EVENT_ACTION_HOVER_MOVE,
    Scroll = ndk_sys::AMOTION_EVENT_ACTION_SCROLL,
    HoverEnter = ndk_sys::AMOTION_EVENT_ACTION_HOVER_ENTER,
    HoverExit = ndk_sys::AMOTION_EVENT_ACTION_HOVER_EXIT,
    ButtonPress = ndk_sys::AMOTION_EVENT_ACTION_BUTTON_PRESS,
    ButtonRelease = ndk_sys::AMOTION_EVENT_ACTION_BUTTON_RELEASE,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

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

/// An axis of a motion event.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-32)
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
pub enum Axis {
    X = ndk_sys::AMOTION_EVENT_AXIS_X,
    Y = ndk_sys::AMOTION_EVENT_AXIS_Y,
    Pressure = ndk_sys::AMOTION_EVENT_AXIS_PRESSURE,
    Size = ndk_sys::AMOTION_EVENT_AXIS_SIZE,
    TouchMajor = ndk_sys::AMOTION_EVENT_AXIS_TOUCH_MAJOR,
    TouchMinor = ndk_sys::AMOTION_EVENT_AXIS_TOUCH_MINOR,
    ToolMajor = ndk_sys::AMOTION_EVENT_AXIS_TOOL_MAJOR,
    ToolMinor = ndk_sys::AMOTION_EVENT_AXIS_TOOL_MINOR,
    Orientation = ndk_sys::AMOTION_EVENT_AXIS_ORIENTATION,
    Vscroll = ndk_sys::AMOTION_EVENT_AXIS_VSCROLL,
    Hscroll = ndk_sys::AMOTION_EVENT_AXIS_HSCROLL,
    Z = ndk_sys::AMOTION_EVENT_AXIS_Z,
    Rx = ndk_sys::AMOTION_EVENT_AXIS_RX,
    Ry = ndk_sys::AMOTION_EVENT_AXIS_RY,
    Rz = ndk_sys::AMOTION_EVENT_AXIS_RZ,
    HatX = ndk_sys::AMOTION_EVENT_AXIS_HAT_X,
    HatY = ndk_sys::AMOTION_EVENT_AXIS_HAT_Y,
    Ltrigger = ndk_sys::AMOTION_EVENT_AXIS_LTRIGGER,
    Rtrigger = ndk_sys::AMOTION_EVENT_AXIS_RTRIGGER,
    Throttle = ndk_sys::AMOTION_EVENT_AXIS_THROTTLE,
    Rudder = ndk_sys::AMOTION_EVENT_AXIS_RUDDER,
    Wheel = ndk_sys::AMOTION_EVENT_AXIS_WHEEL,
    Gas = ndk_sys::AMOTION_EVENT_AXIS_GAS,
    Brake = ndk_sys::AMOTION_EVENT_AXIS_BRAKE,
    Distance = ndk_sys::AMOTION_EVENT_AXIS_DISTANCE,
    Tilt = ndk_sys::AMOTION_EVENT_AXIS_TILT,
    Scroll = ndk_sys::AMOTION_EVENT_AXIS_SCROLL,
    RelativeX = ndk_sys::AMOTION_EVENT_AXIS_RELATIVE_X,
    RelativeY = ndk_sys::AMOTION_EVENT_AXIS_RELATIVE_Y,
    Generic1 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_1,
    Generic2 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_2,
    Generic3 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_3,
    Generic4 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_4,
    Generic5 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_5,
    Generic6 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_6,
    Generic7 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_7,
    Generic8 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_8,
    Generic9 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_9,
    Generic10 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_10,
    Generic11 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_11,
    Generic12 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_12,
    Generic13 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_13,
    Generic14 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_14,
    Generic15 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_15,
    Generic16 = ndk_sys::AMOTION_EVENT_AXIS_GENERIC_16,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// The tool type of a pointer.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-48)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// Implements `Into<u32>` and `From<u32>` for converting to/from Android SDK
/// integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum ToolType {
    /// Unknown tool type.
    ///
    /// This constant is used when the tool type is not known or is not relevant, such as for a trackball or other non-pointing device.
    Unknown = ndk_sys::AMOTION_EVENT_TOOL_TYPE_UNKNOWN,

    /// The tool is a finger.
    Finger = ndk_sys::AMOTION_EVENT_TOOL_TYPE_FINGER,

    /// The tool is a stylus.
    Stylus = ndk_sys::AMOTION_EVENT_TOOL_TYPE_STYLUS,

    ///  The tool is a mouse.
    Mouse = ndk_sys::AMOTION_EVENT_TOOL_TYPE_MOUSE,

    /// The tool is an eraser or a stylus being used in an inverted posture.
    Eraser = ndk_sys::AMOTION_EVENT_TOOL_TYPE_ERASER,

    /// The tool is a palm and should be rejected
    Palm = ndk_sys::AMOTION_EVENT_TOOL_TYPE_PALM,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// A bitfield representing the state of buttons during a motion event.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-33)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ButtonState(pub u32);

impl ButtonState {
    #[inline]
    pub fn primary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_PRIMARY != 0
    }
    #[inline]
    pub fn secondary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_SECONDARY != 0
    }
    #[inline]
    pub fn teriary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_TERTIARY != 0
    }
    #[inline]
    pub fn back(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_BACK != 0
    }
    #[inline]
    pub fn forward(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_FORWARD != 0
    }
    #[inline]
    pub fn stylus_primary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_STYLUS_PRIMARY != 0
    }
    #[inline]
    pub fn stylus_secondary(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_BUTTON_STYLUS_SECONDARY != 0
    }
}

impl From<ndk::event::ButtonState> for ButtonState {
    fn from(value: ndk::event::ButtonState) -> Self {
        Self(value.0)
    }
}

/// A bitfield representing which edges were touched by a motion event.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-31)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EdgeFlags(pub u32);

impl EdgeFlags {
    #[inline]
    pub fn top(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_EDGE_FLAG_TOP != 0
    }
    #[inline]
    pub fn bottom(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_EDGE_FLAG_BOTTOM != 0
    }
    #[inline]
    pub fn left(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_EDGE_FLAG_LEFT != 0
    }
    #[inline]
    pub fn right(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_EDGE_FLAG_RIGHT != 0
    }
}

impl From<ndk::event::EdgeFlags> for EdgeFlags {
    fn from(value: ndk::event::EdgeFlags) -> Self {
        Self(value.0)
    }
}

/// Flags associated with this [`MotionEvent`].
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-30)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MotionEventFlags(pub u32);

impl MotionEventFlags {
    #[inline]
    pub fn window_is_obscured(self) -> bool {
        self.0 & ndk_sys::AMOTION_EVENT_FLAG_WINDOW_IS_OBSCURED != 0
    }
}

impl From<ndk::event::MotionEventFlags> for MotionEventFlags {
    fn from(value: ndk::event::MotionEventFlags) -> Self {
        Self(value.0)
    }
}

/// Key actions.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-27)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// Implements `Into<u32>` and `From<u32>` for converting to/from Android SDK
/// integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum KeyAction {
    Down = ndk_sys::AKEY_EVENT_ACTION_DOWN,
    Up = ndk_sys::AKEY_EVENT_ACTION_UP,
    Multiple = ndk_sys::AKEY_EVENT_ACTION_MULTIPLE,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// Key codes.
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-39)
///
/// # Android Extensible Enum
///
/// This is a runtime [extensible enum](`crate#android-extensible-enums`) and
/// should be handled similar to a `#[non_exhaustive]` enum to maintain
/// forwards compatibility.
///
/// Implements `Into<u32>` and `From<u32>` for converting to/from Android SDK
/// integer values.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[non_exhaustive]
#[repr(u32)]
pub enum Keycode {
    Unknown = ndk_sys::AKEYCODE_UNKNOWN,
    SoftLeft = ndk_sys::AKEYCODE_SOFT_LEFT,
    SoftRight = ndk_sys::AKEYCODE_SOFT_RIGHT,
    Home = ndk_sys::AKEYCODE_HOME,
    Back = ndk_sys::AKEYCODE_BACK,
    Call = ndk_sys::AKEYCODE_CALL,
    Endcall = ndk_sys::AKEYCODE_ENDCALL,
    Keycode0 = ndk_sys::AKEYCODE_0,
    Keycode1 = ndk_sys::AKEYCODE_1,
    Keycode2 = ndk_sys::AKEYCODE_2,
    Keycode3 = ndk_sys::AKEYCODE_3,
    Keycode4 = ndk_sys::AKEYCODE_4,
    Keycode5 = ndk_sys::AKEYCODE_5,
    Keycode6 = ndk_sys::AKEYCODE_6,
    Keycode7 = ndk_sys::AKEYCODE_7,
    Keycode8 = ndk_sys::AKEYCODE_8,
    Keycode9 = ndk_sys::AKEYCODE_9,
    Star = ndk_sys::AKEYCODE_STAR,
    Pound = ndk_sys::AKEYCODE_POUND,
    DpadUp = ndk_sys::AKEYCODE_DPAD_UP,
    DpadDown = ndk_sys::AKEYCODE_DPAD_DOWN,
    DpadLeft = ndk_sys::AKEYCODE_DPAD_LEFT,
    DpadRight = ndk_sys::AKEYCODE_DPAD_RIGHT,
    DpadCenter = ndk_sys::AKEYCODE_DPAD_CENTER,
    VolumeUp = ndk_sys::AKEYCODE_VOLUME_UP,
    VolumeDown = ndk_sys::AKEYCODE_VOLUME_DOWN,
    Power = ndk_sys::AKEYCODE_POWER,
    Camera = ndk_sys::AKEYCODE_CAMERA,
    Clear = ndk_sys::AKEYCODE_CLEAR,
    A = ndk_sys::AKEYCODE_A,
    B = ndk_sys::AKEYCODE_B,
    C = ndk_sys::AKEYCODE_C,
    D = ndk_sys::AKEYCODE_D,
    E = ndk_sys::AKEYCODE_E,
    F = ndk_sys::AKEYCODE_F,
    G = ndk_sys::AKEYCODE_G,
    H = ndk_sys::AKEYCODE_H,
    I = ndk_sys::AKEYCODE_I,
    J = ndk_sys::AKEYCODE_J,
    K = ndk_sys::AKEYCODE_K,
    L = ndk_sys::AKEYCODE_L,
    M = ndk_sys::AKEYCODE_M,
    N = ndk_sys::AKEYCODE_N,
    O = ndk_sys::AKEYCODE_O,
    P = ndk_sys::AKEYCODE_P,
    Q = ndk_sys::AKEYCODE_Q,
    R = ndk_sys::AKEYCODE_R,
    S = ndk_sys::AKEYCODE_S,
    T = ndk_sys::AKEYCODE_T,
    U = ndk_sys::AKEYCODE_U,
    V = ndk_sys::AKEYCODE_V,
    W = ndk_sys::AKEYCODE_W,
    X = ndk_sys::AKEYCODE_X,
    Y = ndk_sys::AKEYCODE_Y,
    Z = ndk_sys::AKEYCODE_Z,
    Comma = ndk_sys::AKEYCODE_COMMA,
    Period = ndk_sys::AKEYCODE_PERIOD,
    AltLeft = ndk_sys::AKEYCODE_ALT_LEFT,
    AltRight = ndk_sys::AKEYCODE_ALT_RIGHT,
    ShiftLeft = ndk_sys::AKEYCODE_SHIFT_LEFT,
    ShiftRight = ndk_sys::AKEYCODE_SHIFT_RIGHT,
    Tab = ndk_sys::AKEYCODE_TAB,
    Space = ndk_sys::AKEYCODE_SPACE,
    Sym = ndk_sys::AKEYCODE_SYM,
    Explorer = ndk_sys::AKEYCODE_EXPLORER,
    Envelope = ndk_sys::AKEYCODE_ENVELOPE,
    Enter = ndk_sys::AKEYCODE_ENTER,
    Del = ndk_sys::AKEYCODE_DEL,
    Grave = ndk_sys::AKEYCODE_GRAVE,
    Minus = ndk_sys::AKEYCODE_MINUS,
    Equals = ndk_sys::AKEYCODE_EQUALS,
    LeftBracket = ndk_sys::AKEYCODE_LEFT_BRACKET,
    RightBracket = ndk_sys::AKEYCODE_RIGHT_BRACKET,
    Backslash = ndk_sys::AKEYCODE_BACKSLASH,
    Semicolon = ndk_sys::AKEYCODE_SEMICOLON,
    Apostrophe = ndk_sys::AKEYCODE_APOSTROPHE,
    Slash = ndk_sys::AKEYCODE_SLASH,
    At = ndk_sys::AKEYCODE_AT,
    Num = ndk_sys::AKEYCODE_NUM,
    Headsethook = ndk_sys::AKEYCODE_HEADSETHOOK,
    Focus = ndk_sys::AKEYCODE_FOCUS,
    Plus = ndk_sys::AKEYCODE_PLUS,
    Menu = ndk_sys::AKEYCODE_MENU,
    Notification = ndk_sys::AKEYCODE_NOTIFICATION,
    Search = ndk_sys::AKEYCODE_SEARCH,
    MediaPlayPause = ndk_sys::AKEYCODE_MEDIA_PLAY_PAUSE,
    MediaStop = ndk_sys::AKEYCODE_MEDIA_STOP,
    MediaNext = ndk_sys::AKEYCODE_MEDIA_NEXT,
    MediaPrevious = ndk_sys::AKEYCODE_MEDIA_PREVIOUS,
    MediaRewind = ndk_sys::AKEYCODE_MEDIA_REWIND,
    MediaFastForward = ndk_sys::AKEYCODE_MEDIA_FAST_FORWARD,
    Mute = ndk_sys::AKEYCODE_MUTE,
    PageUp = ndk_sys::AKEYCODE_PAGE_UP,
    PageDown = ndk_sys::AKEYCODE_PAGE_DOWN,
    Pictsymbols = ndk_sys::AKEYCODE_PICTSYMBOLS,
    SwitchCharset = ndk_sys::AKEYCODE_SWITCH_CHARSET,
    ButtonA = ndk_sys::AKEYCODE_BUTTON_A,
    ButtonB = ndk_sys::AKEYCODE_BUTTON_B,
    ButtonC = ndk_sys::AKEYCODE_BUTTON_C,
    ButtonX = ndk_sys::AKEYCODE_BUTTON_X,
    ButtonY = ndk_sys::AKEYCODE_BUTTON_Y,
    ButtonZ = ndk_sys::AKEYCODE_BUTTON_Z,
    ButtonL1 = ndk_sys::AKEYCODE_BUTTON_L1,
    ButtonR1 = ndk_sys::AKEYCODE_BUTTON_R1,
    ButtonL2 = ndk_sys::AKEYCODE_BUTTON_L2,
    ButtonR2 = ndk_sys::AKEYCODE_BUTTON_R2,
    ButtonThumbl = ndk_sys::AKEYCODE_BUTTON_THUMBL,
    ButtonThumbr = ndk_sys::AKEYCODE_BUTTON_THUMBR,
    ButtonStart = ndk_sys::AKEYCODE_BUTTON_START,
    ButtonSelect = ndk_sys::AKEYCODE_BUTTON_SELECT,
    ButtonMode = ndk_sys::AKEYCODE_BUTTON_MODE,
    Escape = ndk_sys::AKEYCODE_ESCAPE,
    ForwardDel = ndk_sys::AKEYCODE_FORWARD_DEL,
    CtrlLeft = ndk_sys::AKEYCODE_CTRL_LEFT,
    CtrlRight = ndk_sys::AKEYCODE_CTRL_RIGHT,
    CapsLock = ndk_sys::AKEYCODE_CAPS_LOCK,
    ScrollLock = ndk_sys::AKEYCODE_SCROLL_LOCK,
    MetaLeft = ndk_sys::AKEYCODE_META_LEFT,
    MetaRight = ndk_sys::AKEYCODE_META_RIGHT,
    Function = ndk_sys::AKEYCODE_FUNCTION,
    Sysrq = ndk_sys::AKEYCODE_SYSRQ,
    Break = ndk_sys::AKEYCODE_BREAK,
    MoveHome = ndk_sys::AKEYCODE_MOVE_HOME,
    MoveEnd = ndk_sys::AKEYCODE_MOVE_END,
    Insert = ndk_sys::AKEYCODE_INSERT,
    Forward = ndk_sys::AKEYCODE_FORWARD,
    MediaPlay = ndk_sys::AKEYCODE_MEDIA_PLAY,
    MediaPause = ndk_sys::AKEYCODE_MEDIA_PAUSE,
    MediaClose = ndk_sys::AKEYCODE_MEDIA_CLOSE,
    MediaEject = ndk_sys::AKEYCODE_MEDIA_EJECT,
    MediaRecord = ndk_sys::AKEYCODE_MEDIA_RECORD,
    F1 = ndk_sys::AKEYCODE_F1,
    F2 = ndk_sys::AKEYCODE_F2,
    F3 = ndk_sys::AKEYCODE_F3,
    F4 = ndk_sys::AKEYCODE_F4,
    F5 = ndk_sys::AKEYCODE_F5,
    F6 = ndk_sys::AKEYCODE_F6,
    F7 = ndk_sys::AKEYCODE_F7,
    F8 = ndk_sys::AKEYCODE_F8,
    F9 = ndk_sys::AKEYCODE_F9,
    F10 = ndk_sys::AKEYCODE_F10,
    F11 = ndk_sys::AKEYCODE_F11,
    F12 = ndk_sys::AKEYCODE_F12,
    NumLock = ndk_sys::AKEYCODE_NUM_LOCK,
    Numpad0 = ndk_sys::AKEYCODE_NUMPAD_0,
    Numpad1 = ndk_sys::AKEYCODE_NUMPAD_1,
    Numpad2 = ndk_sys::AKEYCODE_NUMPAD_2,
    Numpad3 = ndk_sys::AKEYCODE_NUMPAD_3,
    Numpad4 = ndk_sys::AKEYCODE_NUMPAD_4,
    Numpad5 = ndk_sys::AKEYCODE_NUMPAD_5,
    Numpad6 = ndk_sys::AKEYCODE_NUMPAD_6,
    Numpad7 = ndk_sys::AKEYCODE_NUMPAD_7,
    Numpad8 = ndk_sys::AKEYCODE_NUMPAD_8,
    Numpad9 = ndk_sys::AKEYCODE_NUMPAD_9,
    NumpadDivide = ndk_sys::AKEYCODE_NUMPAD_DIVIDE,
    NumpadMultiply = ndk_sys::AKEYCODE_NUMPAD_MULTIPLY,
    NumpadSubtract = ndk_sys::AKEYCODE_NUMPAD_SUBTRACT,
    NumpadAdd = ndk_sys::AKEYCODE_NUMPAD_ADD,
    NumpadDot = ndk_sys::AKEYCODE_NUMPAD_DOT,
    NumpadComma = ndk_sys::AKEYCODE_NUMPAD_COMMA,
    NumpadEnter = ndk_sys::AKEYCODE_NUMPAD_ENTER,
    NumpadEquals = ndk_sys::AKEYCODE_NUMPAD_EQUALS,
    NumpadLeftParen = ndk_sys::AKEYCODE_NUMPAD_LEFT_PAREN,
    NumpadRightParen = ndk_sys::AKEYCODE_NUMPAD_RIGHT_PAREN,
    VolumeMute = ndk_sys::AKEYCODE_VOLUME_MUTE,
    Info = ndk_sys::AKEYCODE_INFO,
    ChannelUp = ndk_sys::AKEYCODE_CHANNEL_UP,
    ChannelDown = ndk_sys::AKEYCODE_CHANNEL_DOWN,
    ZoomIn = ndk_sys::AKEYCODE_ZOOM_IN,
    ZoomOut = ndk_sys::AKEYCODE_ZOOM_OUT,
    Tv = ndk_sys::AKEYCODE_TV,
    Window = ndk_sys::AKEYCODE_WINDOW,
    Guide = ndk_sys::AKEYCODE_GUIDE,
    Dvr = ndk_sys::AKEYCODE_DVR,
    Bookmark = ndk_sys::AKEYCODE_BOOKMARK,
    Captions = ndk_sys::AKEYCODE_CAPTIONS,
    Settings = ndk_sys::AKEYCODE_SETTINGS,
    TvPower = ndk_sys::AKEYCODE_TV_POWER,
    TvInput = ndk_sys::AKEYCODE_TV_INPUT,
    StbPower = ndk_sys::AKEYCODE_STB_POWER,
    StbInput = ndk_sys::AKEYCODE_STB_INPUT,
    AvrPower = ndk_sys::AKEYCODE_AVR_POWER,
    AvrInput = ndk_sys::AKEYCODE_AVR_INPUT,
    ProgRed = ndk_sys::AKEYCODE_PROG_RED,
    ProgGreen = ndk_sys::AKEYCODE_PROG_GREEN,
    ProgYellow = ndk_sys::AKEYCODE_PROG_YELLOW,
    ProgBlue = ndk_sys::AKEYCODE_PROG_BLUE,
    AppSwitch = ndk_sys::AKEYCODE_APP_SWITCH,
    Button1 = ndk_sys::AKEYCODE_BUTTON_1,
    Button2 = ndk_sys::AKEYCODE_BUTTON_2,
    Button3 = ndk_sys::AKEYCODE_BUTTON_3,
    Button4 = ndk_sys::AKEYCODE_BUTTON_4,
    Button5 = ndk_sys::AKEYCODE_BUTTON_5,
    Button6 = ndk_sys::AKEYCODE_BUTTON_6,
    Button7 = ndk_sys::AKEYCODE_BUTTON_7,
    Button8 = ndk_sys::AKEYCODE_BUTTON_8,
    Button9 = ndk_sys::AKEYCODE_BUTTON_9,
    Button10 = ndk_sys::AKEYCODE_BUTTON_10,
    Button11 = ndk_sys::AKEYCODE_BUTTON_11,
    Button12 = ndk_sys::AKEYCODE_BUTTON_12,
    Button13 = ndk_sys::AKEYCODE_BUTTON_13,
    Button14 = ndk_sys::AKEYCODE_BUTTON_14,
    Button15 = ndk_sys::AKEYCODE_BUTTON_15,
    Button16 = ndk_sys::AKEYCODE_BUTTON_16,
    LanguageSwitch = ndk_sys::AKEYCODE_LANGUAGE_SWITCH,
    MannerMode = ndk_sys::AKEYCODE_MANNER_MODE,
    Keycode3dMode = ndk_sys::AKEYCODE_3D_MODE,
    Contacts = ndk_sys::AKEYCODE_CONTACTS,
    Calendar = ndk_sys::AKEYCODE_CALENDAR,
    Music = ndk_sys::AKEYCODE_MUSIC,
    Calculator = ndk_sys::AKEYCODE_CALCULATOR,
    ZenkakuHankaku = ndk_sys::AKEYCODE_ZENKAKU_HANKAKU,
    Eisu = ndk_sys::AKEYCODE_EISU,
    Muhenkan = ndk_sys::AKEYCODE_MUHENKAN,
    Henkan = ndk_sys::AKEYCODE_HENKAN,
    KatakanaHiragana = ndk_sys::AKEYCODE_KATAKANA_HIRAGANA,
    Yen = ndk_sys::AKEYCODE_YEN,
    Ro = ndk_sys::AKEYCODE_RO,
    Kana = ndk_sys::AKEYCODE_KANA,
    Assist = ndk_sys::AKEYCODE_ASSIST,
    BrightnessDown = ndk_sys::AKEYCODE_BRIGHTNESS_DOWN,
    BrightnessUp = ndk_sys::AKEYCODE_BRIGHTNESS_UP,
    MediaAudioTrack = ndk_sys::AKEYCODE_MEDIA_AUDIO_TRACK,
    Sleep = ndk_sys::AKEYCODE_SLEEP,
    Wakeup = ndk_sys::AKEYCODE_WAKEUP,
    Pairing = ndk_sys::AKEYCODE_PAIRING,
    MediaTopMenu = ndk_sys::AKEYCODE_MEDIA_TOP_MENU,
    Keycode11 = ndk_sys::AKEYCODE_11,
    Keycode12 = ndk_sys::AKEYCODE_12,
    LastChannel = ndk_sys::AKEYCODE_LAST_CHANNEL,
    TvDataService = ndk_sys::AKEYCODE_TV_DATA_SERVICE,
    VoiceAssist = ndk_sys::AKEYCODE_VOICE_ASSIST,
    TvRadioService = ndk_sys::AKEYCODE_TV_RADIO_SERVICE,
    TvTeletext = ndk_sys::AKEYCODE_TV_TELETEXT,
    TvNumberEntry = ndk_sys::AKEYCODE_TV_NUMBER_ENTRY,
    TvTerrestrialAnalog = ndk_sys::AKEYCODE_TV_TERRESTRIAL_ANALOG,
    TvTerrestrialDigital = ndk_sys::AKEYCODE_TV_TERRESTRIAL_DIGITAL,
    TvSatellite = ndk_sys::AKEYCODE_TV_SATELLITE,
    TvSatelliteBs = ndk_sys::AKEYCODE_TV_SATELLITE_BS,
    TvSatelliteCs = ndk_sys::AKEYCODE_TV_SATELLITE_CS,
    TvSatelliteService = ndk_sys::AKEYCODE_TV_SATELLITE_SERVICE,
    TvNetwork = ndk_sys::AKEYCODE_TV_NETWORK,
    TvAntennaCable = ndk_sys::AKEYCODE_TV_ANTENNA_CABLE,
    TvInputHdmi1 = ndk_sys::AKEYCODE_TV_INPUT_HDMI_1,
    TvInputHdmi2 = ndk_sys::AKEYCODE_TV_INPUT_HDMI_2,
    TvInputHdmi3 = ndk_sys::AKEYCODE_TV_INPUT_HDMI_3,
    TvInputHdmi4 = ndk_sys::AKEYCODE_TV_INPUT_HDMI_4,
    TvInputComposite1 = ndk_sys::AKEYCODE_TV_INPUT_COMPOSITE_1,
    TvInputComposite2 = ndk_sys::AKEYCODE_TV_INPUT_COMPOSITE_2,
    TvInputComponent1 = ndk_sys::AKEYCODE_TV_INPUT_COMPONENT_1,
    TvInputComponent2 = ndk_sys::AKEYCODE_TV_INPUT_COMPONENT_2,
    TvInputVga1 = ndk_sys::AKEYCODE_TV_INPUT_VGA_1,
    TvAudioDescription = ndk_sys::AKEYCODE_TV_AUDIO_DESCRIPTION,
    TvAudioDescriptionMixUp = ndk_sys::AKEYCODE_TV_AUDIO_DESCRIPTION_MIX_UP,
    TvAudioDescriptionMixDown = ndk_sys::AKEYCODE_TV_AUDIO_DESCRIPTION_MIX_DOWN,
    TvZoomMode = ndk_sys::AKEYCODE_TV_ZOOM_MODE,
    TvContentsMenu = ndk_sys::AKEYCODE_TV_CONTENTS_MENU,
    TvMediaContextMenu = ndk_sys::AKEYCODE_TV_MEDIA_CONTEXT_MENU,
    TvTimerProgramming = ndk_sys::AKEYCODE_TV_TIMER_PROGRAMMING,
    Help = ndk_sys::AKEYCODE_HELP,
    NavigatePrevious = ndk_sys::AKEYCODE_NAVIGATE_PREVIOUS,
    NavigateNext = ndk_sys::AKEYCODE_NAVIGATE_NEXT,
    NavigateIn = ndk_sys::AKEYCODE_NAVIGATE_IN,
    NavigateOut = ndk_sys::AKEYCODE_NAVIGATE_OUT,
    StemPrimary = ndk_sys::AKEYCODE_STEM_PRIMARY,
    Stem1 = ndk_sys::AKEYCODE_STEM_1,
    Stem2 = ndk_sys::AKEYCODE_STEM_2,
    Stem3 = ndk_sys::AKEYCODE_STEM_3,
    DpadUpLeft = ndk_sys::AKEYCODE_DPAD_UP_LEFT,
    DpadDownLeft = ndk_sys::AKEYCODE_DPAD_DOWN_LEFT,
    DpadUpRight = ndk_sys::AKEYCODE_DPAD_UP_RIGHT,
    DpadDownRight = ndk_sys::AKEYCODE_DPAD_DOWN_RIGHT,
    MediaSkipForward = ndk_sys::AKEYCODE_MEDIA_SKIP_FORWARD,
    MediaSkipBackward = ndk_sys::AKEYCODE_MEDIA_SKIP_BACKWARD,
    MediaStepForward = ndk_sys::AKEYCODE_MEDIA_STEP_FORWARD,
    MediaStepBackward = ndk_sys::AKEYCODE_MEDIA_STEP_BACKWARD,
    SoftSleep = ndk_sys::AKEYCODE_SOFT_SLEEP,
    Cut = ndk_sys::AKEYCODE_CUT,
    Copy = ndk_sys::AKEYCODE_COPY,
    Paste = ndk_sys::AKEYCODE_PASTE,
    SystemNavigationUp = ndk_sys::AKEYCODE_SYSTEM_NAVIGATION_UP,
    SystemNavigationDown = ndk_sys::AKEYCODE_SYSTEM_NAVIGATION_DOWN,
    SystemNavigationLeft = ndk_sys::AKEYCODE_SYSTEM_NAVIGATION_LEFT,
    SystemNavigationRight = ndk_sys::AKEYCODE_SYSTEM_NAVIGATION_RIGHT,
    AllApps = ndk_sys::AKEYCODE_ALL_APPS,
    Refresh = ndk_sys::AKEYCODE_REFRESH,
    ThumbsUp = ndk_sys::AKEYCODE_THUMBS_UP,
    ThumbsDown = ndk_sys::AKEYCODE_THUMBS_DOWN,
    ProfileSwitch = ndk_sys::AKEYCODE_PROFILE_SWITCH,

    #[doc(hidden)]
    #[num_enum(catch_all)]
    __Unknown(u32),
}

/// Flags associated with [`KeyEvent`].
///
/// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#anonymous-enum-28)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KeyEventFlags(pub u32);

impl KeyEventFlags {
    #[inline]
    pub fn cancelled(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_CANCELED != 0
    }
    #[inline]
    pub fn cancelled_long_press(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_CANCELED_LONG_PRESS != 0
    }
    #[inline]
    pub fn editor_action(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_EDITOR_ACTION != 0
    }
    #[inline]
    pub fn fallback(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_FALLBACK != 0
    }
    #[inline]
    pub fn from_system(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_FROM_SYSTEM != 0
    }
    #[inline]
    pub fn keep_touch_mode(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_KEEP_TOUCH_MODE != 0
    }
    #[inline]
    pub fn long_press(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_LONG_PRESS != 0
    }
    #[inline]
    pub fn soft_keyboard(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_SOFT_KEYBOARD != 0
    }
    #[inline]
    pub fn tracking(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_TRACKING != 0
    }
    #[inline]
    pub fn virtual_hard_key(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_VIRTUAL_HARD_KEY != 0
    }
    #[inline]
    pub fn woke_here(&self) -> bool {
        self.0 & ndk_sys::AKEY_EVENT_FLAG_WOKE_HERE != 0
    }
}

impl From<ndk::event::KeyEventFlags> for KeyEventFlags {
    fn from(value: ndk::event::KeyEventFlags) -> Self {
        Self(value.0)
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

/// An exclusive, lending iterator for input events
pub struct InputIterator<'a> {
    pub(crate) inner: crate::activity_impl::InputIteratorInner<'a>,
}

impl<'a> InputIterator<'a> {
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

impl<'a> Pointer<'a> {
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

impl<'a> ExactSizeIterator for PointersIter<'a> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}
