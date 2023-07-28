# async-tcl

Integrate the Tcl/Tk event loop with `async_io` and `async_executor` (the `smol` runtime).

## Guide

```rs
use futures_lite::stream::StreamExt as _;

async_tcl::initialize_notifier();
async_tcl::EXECUTOR.with(|executor| executor.spawn(async_io::Timer::interval(std::time::Duration::from_millis(1)).enumerate().then(|(i, _)| async move { println!("{}", i) }).for_each(drop)).detach()); // Yez
```

That is All
