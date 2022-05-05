use crate::{
	memory::frame::{AllocateHints, OwnedPageFrames, PageFrame},
	object_table::{Error, Object, Root, Ticket},
	scheduler::MemoryObject,
};
use alloc::{boxed::Box, sync::Arc};
use core::{cell::Cell, mem::ManuallyDrop};

/// The table with all the processes running on this system.
pub struct ProcessTable;

impl Object for ProcessTable {
	/// Create a new process.
	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(if path == b"new" {
			Ok(Arc::new(ProcessBuilder::new()))
		} else {
			Err(Error::CantCreateObject)
		})
	}
}

/// A helper structure to create new processes.
struct ProcessBuilder {
	// FIXME Cell is !Sync, so I'm pretty sure this isn't supposed to compile _at all_
	// Some investigation later and it seems we'll have to require Send on quite a few types *sigh*
	bin: Cell<Option<Arc<dyn MemoryObject>>>,
	objects: Cell<arena::Arena<Arc<dyn Object>, u8>>,
	stack: Cell<Option<OwnedPageFrames>>,
	stack_offset: Cell<usize>,
}

impl ProcessBuilder {
	fn new() -> Self {
		Self {
			bin: Cell::new(None),
			objects: Cell::new(Default::default()),
			stack: Cell::new(Some({
				let mut p = OwnedPageFrames::new(
					1.try_into().unwrap(),
					AllocateHints {
						address: 0 as *const _,
						color: 0,
					},
				)
				.unwrap();
				unsafe {
					p.clear();
				}
				p
			})),
			stack_offset: Cell::new(0),
		}
	}
}

impl Object for ProcessBuilder {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(match path {
			b"binary" => Ok(Arc::new(SetBinary { builder: self })),
			b"objects" => Ok(Arc::new(AddObject { builder: self })),
			b"stack" => Ok(Arc::new(SetStack { builder: self })),
			_ => Err(Error::DoesNotExist),
		})
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(if path == b"spawn" {
			Ok({
				super::Process::from_elf(
					self.bin.take().unwrap(),
					self.stack.take().take(),
					0,
					self.objects.take(),
				)
				.unwrap()
			})
		} else {
			Err(Error::CantCreateObject)
		})
	}
}

struct SetBinary {
	builder: Arc<ProcessBuilder>,
}

impl Object for SetBinary {
	fn share(&self, object: &Arc<dyn Object>) -> Ticket<u64> {
		if let Some(object) = object.memory_object(0) {
			self.builder.bin.set(Some(object.into()));
			Ticket::new_complete(Ok(0))
		} else {
			todo!()
		}
	}
}

struct AddObject {
	builder: Arc<ProcessBuilder>,
}

impl Object for AddObject {
	fn share(&self, object: &Arc<dyn Object>) -> Ticket<u64> {
		let mut objs = self.builder.objects.take();
		let h = objs.insert(object.clone());
		self.builder.objects.set(objs);
		Ticket::new_complete(Ok(super::erase_handle(h).into()))
	}
}

struct SetStack {
	builder: Arc<ProcessBuilder>,
}

impl Object for SetStack {
	fn write(&self, _: u64, data: &[u8]) -> Ticket<usize> {
		let stack = self.builder.stack.take().unwrap();
		unsafe {
			stack.write(self.builder.stack_offset.get(), data);
		}
		self.builder.stack.set(Some(stack));
		self.builder
			.stack_offset
			.set(self.builder.stack_offset.get() + data.len());
		Ticket::new_complete(Ok(data.len()))
	}
}

pub fn init(root: &crate::object_table::Root) {
	let tbl = ManuallyDrop::new(Arc::new(ProcessTable) as Arc<dyn Object>);
	root.add(&b"process"[..], Arc::downgrade(&*tbl));
}
