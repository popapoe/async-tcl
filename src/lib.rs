//! Integrate the Tcl/Tk event loop with [`mod@async_io`] and [`mod@async_executor`] (the `smol`
//! runtime).
//!
//! # Guide
//!
//! ```
//! use futures_lite::stream::StreamExt as _;
//!
//! async_tcl::initialize_notifier();
//! async_tcl::EXECUTOR.with(|executor| executor.spawn(async_io::Timer::interval(std::time::Duration::from_millis(1)).enumerate().then(|(i, _)| async move { println!("{}", i) }).for_each(drop)).detach()); // Yez
//! ```
//!
//! That is All

mod ffi;

std::thread_local! {
    /// The executor driven by the Tcl/Tk event loop.
    pub static EXECUTOR: async_executor::LocalExecutor<'static> =
        async_executor::LocalExecutor::new();
}

/// Set the Tcl/Tk notifier to poll [`EXECUTOR`].
pub fn initialize_notifier() {
    let procs: ffi::Tcl_NotifierProcs = ffi::Tcl_NotifierProcs {
        set_timer_proc: Some(set_timer),
        wait_for_event_proc: Some(wait_for_event),
        create_file_handler_proc: Some(create_file_handler),
        delete_file_handler_proc: Some(delete_file_handler),
        init_notifier_proc: None,
        finalize_notifier_proc: None,
        alert_notifier_proc: None,
        service_mode_hook_proc: None,
    };
    unsafe { ffi::Tcl_SetNotifier(&procs as *const ffi::Tcl_NotifierProcs) };
}

std::thread_local! {
    /// The mapping between file descriptors and [`struct@FileHandler`]s.
    static FILE_HANDLERS: std::cell::RefCell<
        std::collections::HashMap<std::ffi::c_int, std::rc::Rc<std::cell::RefCell<FileHandler>>>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
    /// The current timer, set by [`fn@set_timer`].
    static TIMER: std::cell::RefCell<Option<async_task::Task<()>>> = std::cell::RefCell::new(None);
}

/// The state of a file handler created by [`fn@create_file_handler`].
struct FileHandler {
    proc: Option<unsafe extern "C" fn(client_data: *mut std::ffi::c_void, mask: std::ffi::c_int)>,
    client_data: *mut std::ffi::c_void,
    /// The mask of events to listen for.
    mask: std::ffi::c_int,
    /// The mask of events that have occured since the last call to this handler.
    ready: std::ffi::c_int,
    /// The spawned [`struct@FileHandlerFuture`].
    task: async_task::Task<()>,
    /// The waker for the [`struct@FileHandlerFuture`].
    waker: Option<std::task::Waker>,
}

/// A [`Tcl_Event`](struct@ffi::Tcl_Event) for a [`struct@FileHandler`].
///
/// This struct uses type punning.
#[repr(C)]
struct FileHandlerEvent {
    /// The `Tcl_EventProc`.  This is always [`fn@file_handler_event_proc`].
    proc: Option<
        unsafe extern "C" fn(
            ev_ptr: *mut ffi::Tcl_Event,
            flags: std::ffi::c_int,
        ) -> std::ffi::c_int,
    >,
    next_ptr: *mut ffi::Tcl_Event,
    weak: std::rc::Weak<std::cell::RefCell<FileHandler>>,
}
/// The `Tcl_EventProc` for [`struct@FileHandlerEvent`]s.
unsafe extern "C" fn file_handler_event_proc(
    ev_ptr: *mut ffi::Tcl_Event,
    flags: std::ffi::c_int,
) -> std::ffi::c_int {
    if flags & ffi::TCL_FILE_EVENTS == 0 {
        0
    } else {
        let file_event: *mut FileHandlerEvent = ev_ptr as *mut FileHandlerEvent;
        if let Some((Some(proc), client_data, ready)) = {
            if let Some(handler) = (*file_event).weak.upgrade() {
                let handler: std::cell::Ref<'_, FileHandler> = handler.borrow();
                if handler.ready & handler.mask == 0 {
                    None
                } else {
                    Some((handler.proc, handler.client_data, handler.ready))
                }
            } else {
                None
            }
        } {
            proc(client_data, ready);
        }
        if let Some(waker) = {
            if let Some(handler) = (*file_event).weak.upgrade() {
                let mut handler: std::cell::RefMut<'_, FileHandler> = handler.borrow_mut();
                handler.ready = 0;
                handler.waker.clone()
            } else {
                None
            }
        } {
            waker.wake();
        }
        file_event.drop_in_place();
        1
    }
}

/// A future that queues [`struct@FileHandlerEvent`]s when its [`struct@FileHandler`]'s requested
/// events occur.
struct FileHandlerFuture {
    weak: std::rc::Weak<std::cell::RefCell<FileHandler>>,
    file: Option<async_io::Async<std::os::fd::RawFd>>,
}
impl std::future::Future for FileHandlerFuture {
    type Output = ();
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if {
            let handler: std::rc::Rc<std::cell::RefCell<FileHandler>> =
                if let Some(handler) = self.weak.upgrade() {
                    handler
                } else {
                    return std::task::Poll::Ready(());
                };
            let mut handler: std::cell::RefMut<'_, FileHandler> = handler.borrow_mut();
            handler.waker = Some(cx.waker().clone());
            if let Some(ref file) = self.file {
                if handler.mask & ffi::TCL_READABLE != 0 {
                    match file.poll_readable(cx) {
                        std::task::Poll::Pending => {}
                        std::task::Poll::Ready(Ok(())) => handler.ready |= ffi::TCL_READABLE,
                        std::task::Poll::Ready(Err(_)) => handler.ready |= ffi::TCL_EXCEPTION,
                    }
                }
                if handler.mask & ffi::TCL_WRITABLE != 0 {
                    match file.poll_writable(cx) {
                        std::task::Poll::Pending => {}
                        std::task::Poll::Ready(Ok(())) => handler.ready |= ffi::TCL_WRITABLE,
                        std::task::Poll::Ready(Err(_)) => handler.ready |= ffi::TCL_EXCEPTION,
                    }
                }
            } else {
                handler.ready |= ffi::TCL_READABLE;
                handler.ready |= ffi::TCL_WRITABLE;
            }
            handler.ready & handler.mask != 0
        } {
            unsafe {
                let file_event: *mut std::mem::MaybeUninit<FileHandlerEvent> =
                    ffi::Tcl_Alloc(std::mem::size_of::<FileHandlerEvent>())
                        as *mut std::mem::MaybeUninit<FileHandlerEvent>;
                (*file_event).write(FileHandlerEvent {
                    proc: Some(file_handler_event_proc),
                    next_ptr: std::ptr::null_mut(),
                    weak: self.weak.clone(),
                });
                ffi::Tcl_QueueEvent(file_event as *mut ffi::Tcl_Event, ffi::TCL_QUEUE_TAIL);
            }
        }
        std::task::Poll::Pending
    }
}

unsafe extern "C" fn set_timer(time_ptr: *const ffi::Tcl_Time) {
    TIMER.with(|timer| {
        if let Some(old) = {
            let mut timer: std::cell::RefMut<'_, Option<async_task::Task<()>>> = timer.borrow_mut();
            timer.take()
        } {
            EXECUTOR.with(|executor| executor.spawn(old.cancel()).detach());
        }
    });
    if !time_ptr.is_null() {
        if (*time_ptr).sec > 0 || (*time_ptr).usec > 0 {
            let future = {
                let duration: std::time::Duration = std::time::Duration::from(&*time_ptr);
                async move {
                    async_io::Timer::after(duration).await;
                    ffi::Tcl_ServiceAll();
                }
            };
            let task: async_task::Task<()> = EXECUTOR.with(|executor| executor.spawn(future));
            TIMER.with(|timer| *timer.borrow_mut() = Some(task));
        } else {
            ffi::Tcl_ServiceAll();
        }
    }
}
unsafe extern "C" fn wait_for_event(time_ptr: *const ffi::Tcl_Time) -> std::ffi::c_int {
    EXECUTOR.with(|executor| {
        if time_ptr.is_null() {
            async_io::block_on(executor.tick());
            1
        } else if (*time_ptr).sec > 0 || (*time_ptr).usec > 0 {
            let timeout = {
                let duration: std::time::Duration = std::time::Duration::from(&*time_ptr);
                async move {
                    async_io::Timer::after(duration).await;
                }
            };
            async_io::block_on(futures_lite::future::or(executor.tick(), timeout));
            1
        } else if executor.try_tick() {
            1
        } else {
            0
        }
    })
}
unsafe extern "C" fn create_file_handler(
    fd: std::ffi::c_int,
    mask: std::ffi::c_int,
    proc: Option<unsafe extern "C" fn(client_data: *mut std::ffi::c_void, mask: std::ffi::c_int)>,
    client_data: *mut std::ffi::c_void,
) {
    FILE_HANDLERS.with(|file_handlers| {
        match file_handlers.borrow_mut().entry(fd) {
            std::collections::hash_map::Entry::Occupied(occupied) => {
                let handler: &mut std::rc::Rc<std::cell::RefCell<FileHandler>> =
                    occupied.into_mut();
                if let Some(waker) = {
                    let mut handler: std::cell::RefMut<'_, FileHandler> = handler.borrow_mut();
                    let old: std::ffi::c_int = handler.mask;
                    handler.mask = mask;
                    handler.ready = 0;
                    if mask == old {
                        None
                    } else {
                        handler.waker.clone()
                    }
                } {
                    waker.wake();
                }
            }
            std::collections::hash_map::Entry::Vacant(vacant) => {
                vacant.insert(std::rc::Rc::new_cyclic(|weak| {
                    let future: FileHandlerFuture = FileHandlerFuture {
                        weak: weak.clone(),
                        file: async_io::Async::new(fd).ok(),
                    };
                    let task: async_task::Task<()> =
                        EXECUTOR.with(|executor| executor.spawn(future));
                    std::cell::RefCell::new(FileHandler {
                        proc,
                        client_data,
                        mask,
                        ready: 0,
                        task,
                        waker: None,
                    })
                }));
            }
        };
    });
}
unsafe extern "C" fn delete_file_handler(fd: std::ffi::c_int) {
    FILE_HANDLERS.with(|file_handlers| {
        if let Some(handler) = {
            let mut file_handlers: std::cell::RefMut<
                '_,
                std::collections::HashMap<
                    std::ffi::c_int,
                    std::rc::Rc<std::cell::RefCell<FileHandler>>,
                >,
            > = file_handlers.borrow_mut();
            file_handlers.remove(&fd)
        } {
            let task: async_task::Task<()> =
                std::rc::Rc::into_inner(handler).unwrap().into_inner().task;
            EXECUTOR.with(|executor| executor.spawn(task.cancel()).detach());
        }
    });
}
