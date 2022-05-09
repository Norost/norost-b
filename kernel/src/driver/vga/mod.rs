pub mod text;

use crate::sync::Mutex;

pub static TEXT: Mutex<text::Text> = Mutex::new(text::Text::new());

/// # Safety
///
/// This function must be called exactly once.
pub unsafe fn init() {
	TEXT.lock().clear();
}
