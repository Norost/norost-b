pub mod text;

mod table;

pub use text::EmergencyWriter;

use crate::{
	arch::amd64::asm::io::{inb, outb},
	object_table,
	sync::SpinLock,
};
use alloc::sync::Arc;

pub static TEXT: SpinLock<text::Text> = SpinLock::new(text::Text::new());

// From Linux
const VGA_SR_INDEX: u16 = 0x3c4;
const VGA_SR_DATA: u16 = 0x3c5;
const SR01: u8 = 0x1;

/// # Safety
///
/// This function must be called exactly once.
pub unsafe fn init(root: &object_table::Root) {
	TEXT.lock_manual().clear();
	let table = Arc::new(table::VgaTable) as Arc<dyn object_table::Object>;
	root.add(*b"vga", Arc::downgrade(&table));
	let _ = Arc::into_raw(table); // Intentionally leak the table.
}

// https://lkml.kernel.org/lkml/1387300330-8844-1-git-send-email-keithp@keithp.com/
fn is_enabled() -> bool {
	unsafe {
		outb(VGA_SR_INDEX, SR01);
		let sr1 = inb(VGA_SR_DATA);
		sr1 & (1 << 5) == 0
	}
}

fn set_enable(enable: bool) {
	unsafe {
		outb(VGA_SR_INDEX, SR01);
		let sr1 = inb(VGA_SR_DATA);
		outb(VGA_SR_DATA, (sr1 & !(1 << 5)) | u8::from(!enable) << 5);
	}
}
