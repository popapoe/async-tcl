#[link(name = "tcl8.6")]
extern "C" {
    pub fn Tcl_SetNotifier(notifier_proc_ptr: *const Tcl_NotifierProcs);
    pub fn Tcl_Alloc(size: usize) -> *mut std::ffi::c_void;
    pub fn Tcl_QueueEvent(ev_ptr: *mut Tcl_Event, position: std::ffi::c_int);
    pub fn Tcl_ServiceAll() -> std::ffi::c_int;
}

pub const TCL_READABLE: std::ffi::c_int = 1 << 1;
pub const TCL_WRITABLE: std::ffi::c_int = 1 << 2;
pub const TCL_EXCEPTION: std::ffi::c_int = 1 << 3;

pub const TCL_QUEUE_TAIL: std::ffi::c_int = 0;

pub const TCL_FILE_EVENTS: std::ffi::c_int = 1 << 3;

#[repr(C)]
pub struct Tcl_Event {
    pub proc: Option<
        unsafe extern "C" fn(evPtr: *mut Tcl_Event, flags: std::ffi::c_int) -> std::ffi::c_int,
    >,
    pub next_ptr: *mut Tcl_Event,
}
#[repr(C)]
pub struct Tcl_Time {
    pub sec: std::ffi::c_long,
    pub usec: std::ffi::c_long,
}
#[repr(C)]
pub struct Tcl_NotifierProcs {
    pub set_timer_proc: Option<unsafe extern "C" fn(time_ptr: *const Tcl_Time)>,
    pub wait_for_event_proc:
        Option<unsafe extern "C" fn(time_ptr: *const Tcl_Time) -> std::ffi::c_int>,
    pub create_file_handler_proc: Option<
        unsafe extern "C" fn(
            fd: std::ffi::c_int,
            mask: std::ffi::c_int,
            proc: Option<
                unsafe extern "C" fn(client_data: *mut std::ffi::c_void, mask: std::ffi::c_int),
            >,
            client_data: *mut std::ffi::c_void,
        ),
    >,
    pub delete_file_handler_proc: Option<unsafe extern "C" fn(fd: std::ffi::c_int)>,
    pub init_notifier_proc: Option<unsafe extern "C" fn() -> *mut std::ffi::c_void>,
    pub finalize_notifier_proc: Option<unsafe extern "C" fn(client_data: *mut std::ffi::c_void)>,
    pub alert_notifier_proc: Option<unsafe extern "C" fn(client_data: *mut std::ffi::c_void)>,
    pub service_mode_hook_proc: Option<unsafe extern "C" fn(mode: std::ffi::c_int)>,
}

impl From<&Tcl_Time> for std::time::Duration {
    fn from(value: &Tcl_Time) -> std::time::Duration {
        let mut result: std::time::Duration = std::time::Duration::ZERO;
        result += std::time::Duration::from_secs(value.sec as u64);
        result += std::time::Duration::from_micros(value.usec as u64);
        result
    }
}
