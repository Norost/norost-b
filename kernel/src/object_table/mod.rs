//! # Object tables
//!
//! An object table is a collection of objects annotated with a name and any number of tags.
//!
//! Objects can be searched/filtered with tags. Individual objects are addressed by unique
//! integer IDs.

mod job;
mod object;
mod query;
mod streaming;
mod table;
mod ticket;

use crate::scheduler::MemoryObject;
use core::time::Duration;

pub use norostb_kernel::{
	io::{JobId, SeekFrom},
	syscall::Handle,
};

pub use job::*;
pub use object::*;
pub use query::*;
pub use streaming::StreamingTable;
pub use table::*;
pub use ticket::*;
