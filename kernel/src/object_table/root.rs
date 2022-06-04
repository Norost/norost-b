use super::{Error, Object, StreamingTableOwner, Ticket};
use crate::{object_table::QueryIter, sync::SpinLock};
use alloc::{
	boxed::Box,
	collections::BTreeMap,
	sync::{Arc, Weak},
	vec::Vec,
};

/// A root object. This object has multiple child objects which can be accessed by a name, e.g.
///
/// ```
/// net/
/// 	tcp
/// 	...
/// disk/
/// 	data
/// fs/
/// 	bin/
/// 	README/
/// 	...
/// process/
/// ```
pub struct Root {
	objects: SpinLock<BTreeMap<Box<[u8]>, Weak<dyn Object>>>,
}

impl Root {
	/// Create a new root
	pub fn new() -> Self {
		Self {
			objects: SpinLock::new(BTreeMap::new()),
		}
	}

	/// Add a new object to the root.
	pub fn add(&self, name: impl Into<Box<[u8]>>, object: Weak<dyn Object>) {
		self.objects.auto_lock().insert(name.into(), object);
	}

	fn find<'a>(&self, path: &'a [u8]) -> Option<(Arc<dyn Object>, &'a [u8], Option<&'a [u8]>)> {
		let (object, rest) = path
			.iter()
			.position(|c| *c == b'/')
			.map_or((path, None), |i| (&path[..i], Some(&path[i + 1..])));
		let mut objects = self.objects.auto_lock();
		if let Some(obj) = objects.get(object) {
			if let Some(obj) = Weak::upgrade(&obj) {
				Some((obj, object, rest))
			} else {
				objects.remove(object);
				None
			}
		} else {
			None
		}
	}
}

impl Object for Root {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		if path == b"" || path == b"/" {
			Ticket::new_complete(Ok(Arc::new(QueryIter::new(
				self.objects
					.auto_lock()
					.keys()
					.map(|s| s.to_vec())
					.collect::<Vec<_>>()
					.into_iter(),
			))))
		} else {
			self.find(path)
				.map_or_else(not_found, |(obj, _, path)| match path {
					None => Ticket::new_complete(Ok(obj)),
					Some(path) => obj.open(path),
				})
		}
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.find(path).map_or_else(
			|| {
				Ticket::new_complete(if path.contains(&b'/') {
					Err(Error::DoesNotExist)
				} else {
					let mut objects = self.objects.auto_lock();
					let tbl = StreamingTableOwner::new();
					let r = objects.insert(path.into(), StreamingTableOwner::into_inner_weak(&tbl));
					assert!(r.is_none());
					Ok(tbl as Arc<dyn Object>)
				})
			},
			|(obj, _, path)| match path {
				None => Ticket::new_complete(Err(Error::AlreadyExists)),
				Some(path) => obj.create(path),
			},
		)
	}
}

fn not_found<T>() -> Ticket<T> {
	Ticket::new_complete(Err(Error::DoesNotExist))
}
