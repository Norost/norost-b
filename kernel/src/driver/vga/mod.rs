pub mod text;

pub use text::EmergencyWriter;

use crate::sync::SpinLock;

pub static TEXT: SpinLock<text::Text> = SpinLock::new(text::Text::new());

/// # Safety
///
/// This function must be called exactly once.
pub unsafe fn init() {
	TEXT.lock_manual().clear();
}
