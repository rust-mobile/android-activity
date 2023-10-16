// GameActivity handles input events by double buffering MotionEvent and KeyEvent structs which
// essentially just mirror state from the corresponding Java objects.
//
// See also the javadocs for
// [`android.view.InputEvent`](https://developer.android.com/reference/android/view/InputEvent.html),
// [`android.view.MotionEvent`](https://developer.android.com/reference/android/view/MotionEvent.html),
// and [`android.view.KeyEvent`](https://developer.android.com/reference/android/view/KeyEvent).
//
// This code is mostly based on https://github.com/rust-windowing/android-ndk-rs/blob/master/ndk/src/event.rs
//
// The `Source` enum was defined based on the Java docs since there are a couple of source types that
// aren't exposed via the AInputQueue API
// The `Class` was also bound differently to `android-ndk-rs` considering how the class is defined
// by masking bits from the `Source`.

use crate::activity_impl::ffi::{GameActivityKeyEvent, GameActivityMotionEvent};
use crate::input::{
    Axis, Button, ButtonState, EdgeFlags, KeyAction, KeyEventFlags, Keycode, MetaState,
    MotionAction, MotionEventFlags, Pointer, PointersIter, Source, ToolType,
};

// Note: try to keep this wrapper API compatible with the AInputEvent API if possible

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum InputEvent<'a> {
    MotionEvent(MotionEvent<'a>),
    KeyEvent(KeyEvent<'a>),
    TextEvent(crate::input::TextInputState),
}

/// A motion event.
///
/// For general discussion of motion events in Android, see [the relevant
/// javadoc](https://developer.android.com/reference/android/view/MotionEvent).
#[derive(Clone, Debug)]
pub struct MotionEvent<'a> {
    ga_event: &'a GameActivityMotionEvent,
}

impl<'a> MotionEvent<'a> {
    pub(crate) fn new(ga_event: &'a GameActivityMotionEvent) -> Self {
        Self { ga_event }
    }

    /// Get the source of the event.
    ///
    #[inline]
    pub fn source(&self) -> Source {
        let source = self.ga_event.source as u32;
        source.into()
    }

    /// Get the device id associated with the event.
    ///
    #[inline]
    pub fn device_id(&self) -> i32 {
        self.ga_event.deviceId
    }

    /// Returns the motion action associated with the event.
    ///
    /// See [the MotionEvent docs](https://developer.android.com/reference/android/view/MotionEvent#getActionMasked())
    #[inline]
    pub fn action(&self) -> MotionAction {
        let action = self.ga_event.action as u32 & ndk_sys::AMOTION_EVENT_ACTION_MASK;
        action.into()
    }

    /// Returns which button has been modified during a press or release action.
    ///
    /// For actions other than [`MotionAction::ButtonPress`] and
    /// [`MotionAction::ButtonRelease`] the returned value is undefined.
    ///
    /// See [the MotionEvent docs](https://developer.android.com/reference/android/view/MotionEvent#getActionButton())
    #[inline]
    pub fn action_button(&self) -> Button {
        let action = self.ga_event.actionButton as u32;
        action.into()
    }

    /// Returns the pointer index of an `Up` or `Down` event.
    ///
    /// Pointer indices can change per motion event.  For an identifier that stays the same, see
    /// [`Pointer::pointer_id()`].
    ///
    /// This only has a meaning when the [action](self::action) is one of [`Up`](MotionAction::Up),
    /// [`Down`](MotionAction::Down), [`PointerUp`](MotionAction::PointerUp),
    /// or [`PointerDown`](MotionAction::PointerDown).
    #[inline]
    pub fn pointer_index(&self) -> usize {
        let action = self.ga_event.action as u32;
        let index = (action & ndk_sys::AMOTION_EVENT_ACTION_POINTER_INDEX_MASK)
            >> ndk_sys::AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT;
        index as usize
    }

    /*
    /// Returns the pointer id associated with the given pointer index.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getpointerid)
    // TODO: look at output with out-of-range pointer index
    // Probably -1 though
    pub fn pointer_id_for(&self.ga_event, pointer_index: usize) -> i32 {
        unsafe { ndk_sys::AMotionEvent_getPointerId(self.ga_event.ptr.as_ptr(), pointer_index) }
    }
    */

    /// Returns the number of pointers in this event
    ///
    /// See [the MotionEvent docs](https://developer.android.com/reference/android/view/MotionEvent#getPointerCount())
    #[inline]
    pub fn pointer_count(&self) -> usize {
        self.ga_event.pointerCount as usize
    }

    /// An iterator over the pointers in this motion event
    #[inline]
    pub fn pointers(&self) -> PointersIter<'_> {
        PointersIter {
            inner: PointersIterImpl {
                event: self,
                next_index: 0,
                count: self.pointer_count(),
            },
        }
    }

    /// The pointer at a given pointer index. Panics if the pointer index is out of bounds.
    ///
    /// If you need to loop over all the pointers, prefer the [`pointers()`](self::pointers) method.
    #[inline]
    pub fn pointer_at_index(&self, index: usize) -> Pointer<'_> {
        if index >= self.pointer_count() {
            panic!("Pointer index {} is out of bounds", index);
        }
        Pointer {
            inner: PointerImpl { event: self, index },
        }
    }

    /*
    /// Returns the size of the history contained in this event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_gethistorysize)
    #[inline]
    pub fn history_size(&self) -> usize {
        unsafe { ndk_sys::AMotionEvent_getHistorySize(self.ga_event.ptr.as_ptr()) as usize }
    }

    /// An iterator over the historical events contained in this event.
    #[inline]
    pub fn history(&self) -> HistoricalMotionEventsIter<'_> {
        HistoricalMotionEventsIter {
            event: self.ga_event.ptr,
            next_history_index: 0,
            history_size: self.history_size(),
            _marker: std::marker::PhantomData,
        }
    }
    */

    /// Returns the state of any modifier keys that were pressed during the event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getmetastate)
    #[inline]
    pub fn meta_state(&self) -> MetaState {
        MetaState(self.ga_event.metaState as u32)
    }

    /// Returns the button state during this event, as a bitfield.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getbuttonstate)
    #[inline]
    pub fn button_state(&self) -> ButtonState {
        ButtonState(self.ga_event.buttonState as u32)
    }

    /// Returns the time of the start of this gesture, in the `java.lang.System.nanoTime()` time
    /// base
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getdowntime)
    #[inline]
    pub fn down_time(&self) -> i64 {
        self.ga_event.downTime
    }

    /// Returns a bitfield indicating which edges were touched by this event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getedgeflags)
    #[inline]
    pub fn edge_flags(&self) -> EdgeFlags {
        EdgeFlags(self.ga_event.edgeFlags as u32)
    }

    /// Returns the time of this event, in the `java.lang.System.nanoTime()` time base
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_geteventtime)
    #[inline]
    pub fn event_time(&self) -> i64 {
        self.ga_event.eventTime
    }

    /// The flags associated with a motion event.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getflags)
    #[inline]
    pub fn flags(&self) -> MotionEventFlags {
        MotionEventFlags(self.ga_event.flags as u32)
    }

    /* Missing from GameActivity currently...
    /// Returns the offset in the x direction between the coordinates and the raw coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getxoffset)
    #[inline]
    pub fn x_offset(&self.ga_event) -> f32 {
        self.ga_event.x_offset
    }

    /// Returns the offset in the y direction between the coordinates and the raw coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getyoffset)
    #[inline]
    pub fn y_offset(&self.ga_event) -> f32 {
        self.ga_event.y_offset
    }
    */

    /// Returns the precision of the x value of the coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getxprecision)
    #[inline]
    pub fn x_precision(&self) -> f32 {
        self.ga_event.precisionX
    }

    /// Returns the precision of the y value of the coordinates
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_getyprecision)
    #[inline]
    pub fn y_precision(&self) -> f32 {
        self.ga_event.precisionY
    }
}

/// A view into the data of a specific pointer in a motion event.
#[derive(Debug)]
pub(crate) struct PointerImpl<'a> {
    event: &'a MotionEvent<'a>,
    index: usize,
}

impl<'a> PointerImpl<'a> {
    #[inline]
    pub fn pointer_index(&self) -> usize {
        self.index
    }

    #[inline]
    pub fn pointer_id(&self) -> i32 {
        let pointer = &self.event.ga_event.pointers[self.index];
        pointer.id
    }

    #[inline]
    pub fn axis_value(&self, axis: Axis) -> f32 {
        let pointer = &self.event.ga_event.pointers[self.index];
        let axis: u32 = axis.into();
        pointer.axisValues[axis as usize]
    }

    #[inline]
    pub fn raw_x(&self) -> f32 {
        let pointer = &self.event.ga_event.pointers[self.index];
        pointer.rawX
    }

    #[inline]
    pub fn raw_y(&self) -> f32 {
        let pointer = &self.event.ga_event.pointers[self.index];
        pointer.rawY
    }

    #[inline]
    pub fn tool_type(&self) -> ToolType {
        let pointer = &self.event.ga_event.pointers[self.index];
        let tool_type = pointer.toolType as u32;
        tool_type.into()
    }
}

/// An iterator over the pointers in a [`MotionEvent`].
#[derive(Debug)]
pub(crate) struct PointersIterImpl<'a> {
    event: &'a MotionEvent<'a>,
    next_index: usize,
    count: usize,
}

impl<'a> Iterator for PointersIterImpl<'a> {
    type Item = Pointer<'a>;
    fn next(&mut self) -> Option<Pointer<'a>> {
        if self.next_index < self.count {
            let ptr = Pointer {
                inner: PointerImpl {
                    event: self.event,
                    index: self.next_index,
                },
            };
            self.next_index += 1;
            Some(ptr)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.count - self.next_index;
        (size, Some(size))
    }
}

impl<'a> ExactSizeIterator for PointersIterImpl<'a> {
    fn len(&self) -> usize {
        self.count - self.next_index
    }
}

/*
/// Represents a view into a past moment of a motion event
#[derive(Debug)]
pub struct HistoricalMotionEvent<'a> {
    event: NonNull<ndk_sys::AInputEvent>,
    history_index: usize,
    _marker: std::marker::PhantomData<&'a MotionEvent>,
}

// TODO: thread safety?

impl<'a> HistoricalMotionEvent<'a> {
    /// Returns the "history index" associated with this historical event.  Older events have smaller indices.
    #[inline]
    pub fn history_index(&self) -> usize {
        self.history_index
    }

    /// Returns the time of the historical event, in the `java.lang.System.nanoTime()` time base
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#amotionevent_gethistoricaleventtime)
    #[inline]
    pub fn event_time(&self) -> i64 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalEventTime(
                self.event.as_ptr(),
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    /// An iterator over the pointers of this historical motion event
    #[inline]
    pub fn pointers(&self) -> HistoricalPointersIter<'a> {
        HistoricalPointersIter {
            event: self.event,
            history_index: self.history_index,
            next_pointer_index: 0,
            pointer_count: unsafe {
                ndk_sys::AMotionEvent_getPointerCount(self.event.as_ptr()) as usize
            },
            _marker: std::marker::PhantomData,
        }
    }
}

/// An iterator over all the historical moments in a [`MotionEvent`].
///
/// It iterates from oldest to newest.
#[derive(Debug)]
pub struct HistoricalMotionEventsIter<'a> {
    event: NonNull<ndk_sys::AInputEvent>,
    next_history_index: usize,
    history_size: usize,
    _marker: std::marker::PhantomData<&'a MotionEvent>,
}

// TODO: thread safety?

impl<'a> Iterator for HistoricalMotionEventsIter<'a> {
    type Item = HistoricalMotionEvent<'a>;

    fn next(&mut self) -> Option<HistoricalMotionEvent<'a>> {
        if self.next_history_index < self.history_size {
            let res = HistoricalMotionEvent {
                event: self.event,
                history_index: self.next_history_index,
                _marker: std::marker::PhantomData,
            };
            self.next_history_index += 1;
            Some(res)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.history_size - self.next_history_index;
        (size, Some(size))
    }
}
impl ExactSizeIterator for HistoricalMotionEventsIter<'_> {
    fn len(&self) -> usize {
        self.history_size - self.next_history_index
    }
}
impl<'a> DoubleEndedIterator for HistoricalMotionEventsIter<'a> {
    fn next_back(&mut self) -> Option<HistoricalMotionEvent<'a>> {
        if self.next_history_index < self.history_size {
            self.history_size -= 1;
            Some(HistoricalMotionEvent {
                event: self.event,
                history_index: self.history_size,
                _marker: std::marker::PhantomData,
            })
        } else {
            None
        }
    }
}

/// A view into a pointer at a historical moment
#[derive(Debug)]
pub struct HistoricalPointer<'a> {
    event: NonNull<ndk_sys::AInputEvent>,
    pointer_index: usize,
    history_index: usize,
    _marker: std::marker::PhantomData<&'a MotionEvent>,
}

// TODO: thread safety?

impl<'a> HistoricalPointer<'a> {
    #[inline]
    pub fn pointer_index(&self) -> usize {
        self.pointer_index
    }

    #[inline]
    pub fn pointer_id(&self) -> i32 {
        unsafe {
            ndk_sys::AMotionEvent_getPointerId(self.event.as_ptr(), self.pointer_index as ndk_sys::size_t)
        }
    }

    #[inline]
    pub fn history_index(&self) -> usize {
        self.history_index
    }

    #[inline]
    pub fn axis_value(&self, axis: Axis) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalAxisValue(
                self.event.as_ptr(),
                axis as i32,
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn orientation(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalOrientation(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn pressure(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalPressure(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn raw_x(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalRawX(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn raw_y(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalRawY(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn x(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalX(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn y(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalY(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn size(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalSize(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn tool_major(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalToolMajor(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn tool_minor(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalToolMinor(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn touch_major(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalTouchMajor(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }

    #[inline]
    pub fn touch_minor(&self) -> f32 {
        unsafe {
            ndk_sys::AMotionEvent_getHistoricalTouchMinor(
                self.event.as_ptr(),
                self.pointer_index as ndk_sys::size_t,
                self.history_index as ndk_sys::size_t,
            )
        }
    }
}

/// An iterator over the pointers in a historical motion event
#[derive(Debug)]
pub struct HistoricalPointersIter<'a> {
    event: NonNull<ndk_sys::AInputEvent>,
    history_index: usize,
    next_pointer_index: usize,
    pointer_count: usize,
    _marker: std::marker::PhantomData<&'a MotionEvent>,
}

// TODO: thread safety?

impl<'a> Iterator for HistoricalPointersIter<'a> {
    type Item = HistoricalPointer<'a>;

    fn next(&mut self) -> Option<HistoricalPointer<'a>> {
        if self.next_pointer_index < self.pointer_count {
            let ptr = HistoricalPointer {
                event: self.event,
                history_index: self.history_index,
                pointer_index: self.next_pointer_index,
                _marker: std::marker::PhantomData,
            };
            self.next_pointer_index += 1;
            Some(ptr)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.pointer_count - self.next_pointer_index;
        (size, Some(size))
    }
}
impl ExactSizeIterator for HistoricalPointersIter<'_> {
    fn len(&self) -> usize {
        self.pointer_count - self.next_pointer_index
    }
}

*/

/// A key event.
///
/// For general discussion of key events in Android, see [the relevant
/// javadoc](https://developer.android.com/reference/android/view/KeyEvent).
#[derive(Debug, Clone)]
pub struct KeyEvent<'a> {
    ga_event: &'a GameActivityKeyEvent,
}

impl<'a> KeyEvent<'a> {
    pub(crate) fn new(ga_event: &'a GameActivityKeyEvent) -> Self {
        Self { ga_event }
    }

    /// Get the source of the event.
    ///
    #[inline]
    pub fn source(&self) -> Source {
        let source = self.ga_event.source as u32;
        source.into()
    }

    /// Get the device id associated with the event.
    ///
    #[inline]
    pub fn device_id(&self) -> i32 {
        self.ga_event.deviceId
    }

    /// Returns the key action associated with the event.
    ///
    /// See [the KeyEvent docs](https://developer.android.com/reference/android/view/KeyEvent#getAction())
    #[inline]
    pub fn action(&self) -> KeyAction {
        let action = self.ga_event.action as u32;
        action.into()
    }

    #[inline]
    pub fn action_button(&self) -> KeyAction {
        let action = self.ga_event.action as u32;
        action.into()
    }

    /// Returns the last time the key was pressed.  This is on the scale of
    /// `java.lang.System.nanoTime()`, which has nanosecond precision, but no defined start time.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getdowntime)
    #[inline]
    pub fn down_time(&self) -> i64 {
        self.ga_event.downTime
    }

    /// Returns the time this event occured.  This is on the scale of
    /// `java.lang.System.nanoTime()`, which has nanosecond precision, but no defined start time.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_geteventtime)
    #[inline]
    pub fn event_time(&self) -> i64 {
        self.ga_event.eventTime
    }

    /// Returns the keycode associated with this key event
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getkeycode)
    #[inline]
    pub fn key_code(&self) -> Keycode {
        let keycode = self.ga_event.keyCode as u32;
        keycode.into()
    }

    /// Returns the number of repeats of a key.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getrepeatcount)
    #[inline]
    pub fn repeat_count(&self) -> i32 {
        self.ga_event.repeatCount
    }

    /// Returns the hardware keycode of a key.  This varies from device to device.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getscancode)
    #[inline]
    pub fn scan_code(&self) -> i32 {
        self.ga_event.scanCode
    }
}

impl<'a> KeyEvent<'a> {
    /// Flags associated with this [`KeyEvent`].
    ///
    /// See [the NDK docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getflags)
    #[inline]
    pub fn flags(&self) -> KeyEventFlags {
        KeyEventFlags(self.ga_event.flags as u32)
    }

    /// Returns the state of the modifiers during this key event, represented by a bitmask.
    ///
    /// See [the NDK
    /// docs](https://developer.android.com/ndk/reference/group/input#akeyevent_getmetastate)
    #[inline]
    pub fn meta_state(&self) -> MetaState {
        MetaState(self.ga_event.metaState as u32)
    }
}
