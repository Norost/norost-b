//! Default global configuration for the runtime.

#![no_std]
#![feature(alloc_error_handler)]

#[global_allocator]
static ALLOC: rt_alloc::Allocator = rt_alloc::Allocator;

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
	let _ = rt::io::stderr().map(|o| {
		writeln!(
			o,
			"allocation failed for size {}, alignment {}",
			layout.size(),
			layout.align()
		)
	});
	rt::exit(129)
}

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
	let _ = rt::io::stderr().map(|o| writeln!(o, "{}", info));
	rt::exit(128)
}
