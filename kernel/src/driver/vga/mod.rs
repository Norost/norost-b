pub mod text;

pub use text::EmergencyWriter;

use crate::{object_table, sync::SpinLock};

pub static TEXT: SpinLock<text::Text> = SpinLock::new(text::Text::new());

/// # Safety
///
/// This function must be called exactly once.
pub unsafe fn init(_: &object_table::Root) {
	TEXT.lock_manual().clear();
}
