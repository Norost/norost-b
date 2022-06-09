//! Common I/O utilities.

#[cfg(feature = "std")]
mod buf_block;
#[cfg(feature = "std")]
mod cache_seek;
#[cfg(feature = "std")]
pub mod monitor;
pub mod queue;

#[cfg(feature = "std")]
pub use buf_block::BufBlock;
#[cfg(feature = "std")]
pub use cache_seek::CacheSeek;
#[cfg(feature = "std")]
pub use monitor::Monitor;
