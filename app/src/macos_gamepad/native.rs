//! Read-only IOHID full-report listener. All CF/IOKit objects stay on its thread.

use std::{
    collections::HashMap,
    ffi::{c_char, c_void},
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
};

use super::report::{PAYLOAD_LENGTH, PRODUCT_ID, REPORT_ID, State, VENDOR_ID};

type Object = *const c_void;
type ValueCallback = unsafe extern "C" fn(*mut c_void, i32, *mut c_void, Object);
type RemovalCallback = unsafe extern "C" fn(*mut c_void, i32, *mut c_void, Object);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: Object;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    fn CFStringCreateWithCString(allocator: Object, string: *const c_char, encoding: u32)
    -> Object;
    fn CFNumberCreate(allocator: Object, kind: isize, value: *const c_void) -> Object;
    fn CFDictionaryCreate(
        allocator: Object,
        keys: *const Object,
        values: *const Object,
        count: isize,
        key_callbacks: Object,
        value_callbacks: Object,
    ) -> Object;
    fn CFRelease(object: Object);
    fn CFRunLoopGetCurrent() -> Object;
    fn CFRunLoopRunInMode(mode: Object, seconds: f64, return_after_source: u8) -> i32;
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDManagerCreate(allocator: Object, options: u32) -> Object;
    fn IOHIDManagerSetDeviceMatching(manager: Object, matching: Object);
    fn IOHIDManagerRegisterInputValueCallback(
        manager: Object,
        callback: Option<ValueCallback>,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterDeviceRemovalCallback(
        manager: Object,
        callback: Option<RemovalCallback>,
        context: *mut c_void,
    );
    fn IOHIDManagerScheduleWithRunLoop(manager: Object, run_loop: Object, mode: Object);
    fn IOHIDManagerUnscheduleFromRunLoop(manager: Object, run_loop: Object, mode: Object);
    fn IOHIDManagerOpen(manager: Object, options: u32) -> i32;
    fn IOHIDManagerClose(manager: Object, options: u32) -> i32;
    fn IOHIDValueGetElement(value: Object) -> Object;
    fn IOHIDValueGetLength(value: Object) -> isize;
    fn IOHIDValueGetBytePtr(value: Object) -> *const u8;
    fn IOHIDElementGetDevice(element: Object) -> Object;
    fn IOHIDElementGetUsagePage(element: Object) -> u32;
    fn IOHIDElementGetUsage(element: Object) -> u32;
    fn IOHIDElementGetReportID(element: Object) -> u32;
}

/// Native pointer identities are used only as opaque keys, never dereferenced off-thread.
pub(super) enum Event {
    State(usize, State),
    Removed(usize),
}

struct Context {
    sender: Sender<Event>,
    previous: HashMap<usize, State>,
}

/// Owns a stoppable listener; dropping it closes HID before joining the worker.
pub(super) struct Listener {
    pub receiver: std::sync::Mutex<Receiver<Event>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Listener {
    #[cfg(test)]
    pub(super) fn test_channel() -> (Sender<Event>, Self) {
        let (sender, receiver) = mpsc::channel();
        (
            sender,
            Self {
                receiver: std::sync::Mutex::new(receiver),
                stop: Arc::new(AtomicBool::new(false)),
                worker: None,
            },
        )
    }

    pub fn start() -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("switch-hid".into())
            .spawn(move || {
                // SAFETY: Every object is created, scheduled, accessed and released on this thread.
                unsafe { listen(sender, worker_stop) };
            })?;
        Ok(Self {
            receiver: std::sync::Mutex::new(receiver),
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Callbacks are serialized by the worker's run loop; unchanged timer packets are ignored.
unsafe extern "C" fn input(context: *mut c_void, result: i32, _: *mut c_void, value: Object) {
    if result != 0 || value.is_null() {
        return;
    }
    // SAFETY: IOKit supplies live values; context points to the context in `listen`.
    unsafe {
        let element = IOHIDValueGetElement(value);
        if IOHIDElementGetUsagePage(element) != 0xff01
            || IOHIDElementGetUsage(element) != REPORT_ID
            || IOHIDElementGetReportID(element) != REPORT_ID
            || IOHIDValueGetLength(value) != PAYLOAD_LENGTH as isize
        {
            return;
        }
        let bytes = IOHIDValueGetBytePtr(value);
        if bytes.is_null() {
            return;
        }
        let Some(state) =
            State::decode(REPORT_ID, std::slice::from_raw_parts(bytes, PAYLOAD_LENGTH))
        else {
            return;
        };
        let device = IOHIDElementGetDevice(element);
        if device.is_null() {
            return;
        }
        let context = &mut *context.cast::<Context>();
        let device = device as usize;
        if context.previous.insert(device, state) == Some(state) {
            return;
        }
        let _ = context.sender.send(Event::State(device, state));
    }
}

/// Removal uses the same IOHIDDevice identity as input, including reconnects.
unsafe extern "C" fn removed(context: *mut c_void, _: i32, _: *mut c_void, device: Object) {
    // SAFETY: Callback is registered only while the stack-owned context is live.
    let context = unsafe { &mut *context.cast::<Context>() };
    context.previous.remove(&(device as usize));
    let _ = context.sender.send(Event::Removed(device as usize));
}

/// Return an owning dictionary reference, releasing temporary keys and numbers.
unsafe fn matching() -> Object {
    // SAFETY: CF receives valid NUL-terminated strings and pointers to i32 numbers.
    unsafe {
        let vendor = i32::from(VENDOR_ID);
        let product = i32::from(PRODUCT_ID);
        let keys = [
            CFStringCreateWithCString(ptr::null(), c"VendorID".as_ptr(), 0x08000100),
            CFStringCreateWithCString(ptr::null(), c"ProductID".as_ptr(), 0x08000100),
        ];
        // kCFNumberSInt32Type = 3; CF copies the pointed-to numbers.
        let values = [
            CFNumberCreate(ptr::null(), 3, ptr::from_ref(&vendor).cast()),
            CFNumberCreate(ptr::null(), 3, ptr::from_ref(&product).cast()),
        ];
        let dictionary = if keys
            .iter()
            .chain(values.iter())
            .any(|value| value.is_null())
        {
            ptr::null()
        } else {
            CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                keys.len() as isize,
                ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
                ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
            )
        };
        for object in keys.into_iter().chain(values) {
            if !object.is_null() {
                CFRelease(object);
            }
        }
        dictionary
    }
}

/// Observe already-streaming full reports without seizing devices or changing modes.
unsafe fn listen(sender: Sender<Event>, stop: Arc<AtomicBool>) {
    // SAFETY: Balanced create/release, schedule/unschedule and open/close on one thread.
    unsafe {
        let manager = IOHIDManagerCreate(ptr::null(), 0);
        if manager.is_null() {
            return;
        }
        let filter = matching();
        if filter.is_null() {
            CFRelease(manager);
            return;
        }
        IOHIDManagerSetDeviceMatching(manager, filter);
        CFRelease(filter);
        let mut state = Context {
            sender,
            previous: HashMap::new(),
        };
        let context = ptr::from_mut(&mut state).cast();
        IOHIDManagerRegisterInputValueCallback(manager, Some(input), context);
        IOHIDManagerRegisterDeviceRemovalCallback(manager, Some(removed), context);
        let run_loop = CFRunLoopGetCurrent();
        IOHIDManagerScheduleWithRunLoop(manager, run_loop, kCFRunLoopDefaultMode);
        let status = IOHIDManagerOpen(manager, 0);
        if status == 0 {
            while !stop.load(Ordering::Acquire) {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 0);
            }
            IOHIDManagerClose(manager, 0);
        } else {
            bevy::log::warn!(status, "Switch full-report HID listener could not open");
        }
        IOHIDManagerUnscheduleFromRunLoop(manager, run_loop, kCFRunLoopDefaultMode);
        IOHIDManagerRegisterInputValueCallback(manager, None, ptr::null_mut());
        IOHIDManagerRegisterDeviceRemovalCallback(manager, None, ptr::null_mut());
        CFRelease(manager);
    }
}
