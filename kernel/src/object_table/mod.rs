//! # Object tables
//!
//! An object table is a collection of objects annotated with a name and any number of tags.
//!
//! Objects can be searched/filtered with tags. Individual objects are addressed by unique
//! integer IDs.

pub mod message_pipe;
pub mod pipe;

mod object;
mod query;
mod root;
mod streaming;
mod subrange;
mod ticket;

pub use crate::scheduler::{MemoryObject, PageFlags};

pub use norostb_kernel::{
	error::Error,
	io::{SeekFrom, TinySlice},
	syscall::Handle,
};

pub use {
	object::*,
	query::*,
	root::Root,
	streaming::{NewStreamingTableError, StreamingTable},
	subrange::SubRange,
	ticket::*,
};
