//! Default global configuration for the runtime.

#![no_std]
#![feature(alloc_error_handler)]

#[global_allocator]
static ALLOC: rt_alloc::Allocator = rt_alloc::Allocator;

fn name() -> &'static str {
	rt::args::args()
		.next()
		.and_then(|s| core::str::from_utf8(s).ok())
		.unwrap_or("??")
}

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
	let _ = rt::io::stderr().map(|o| {
		writeln!(
			o,
			"{}: allocation failed for size {}, alignment {}",
			name(),
			layout.size(),
			layout.align()
		)
	});
	rt::exit(129)
}

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
	let _ = rt::io::stderr().map(|o| writeln!(o, "{}: {}", name(), info));
	rt::exit(128)
}
