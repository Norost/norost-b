use super::{Error, Object, Query, QueryResult, StreamingTable, Ticket};
use crate::sync::Mutex;
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
	objects: Mutex<BTreeMap<Box<[u8]>, Weak<dyn Object>>>,
}

impl Root {
	/// Create a new root
	pub fn new() -> Self {
		Self {
			objects: Mutex::new(BTreeMap::new()),
		}
	}

	/// Add a new object to the root.
	pub fn add(&self, name: impl Into<Box<[u8]>>, object: Weak<dyn Object>) {
		self.objects.lock_unchecked().insert(name.into(), object);
	}

	fn find<'a>(&self, path: &'a [u8]) -> Option<(Arc<dyn Object>, &'a [u8], &'a [u8])> {
		let (object, rest) = path
			.iter()
			.position(|c| *c == b'/')
			.map_or((path, &b""[..]), |i| (&path[..i], &path[i + 1..]));
		let mut objects = self.objects.lock();
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
	fn query(self: Arc<Self>, mut prefix: Vec<u8>, filter: &[u8]) -> Ticket<Box<dyn Query>> {
		if filter == b"" || filter == b"/" {
			struct Q<I: Iterator<Item = Ticket<QueryResult>>>(I);
			impl<I: Iterator<Item = Ticket<QueryResult>>> Iterator for Q<I> {
				type Item = Ticket<QueryResult>;
				fn next(&mut self) -> Option<Self::Item> {
					self.0.next()
				}
			}
			impl<I: Iterator<Item = Ticket<QueryResult>>> Query for Q<I> {}

			// Filter any dead objects before querying.
			let mut objects = self.objects.lock();
			objects.retain(|_, v| v.strong_count() > 0);
			Ticket::new_complete(Ok(Box::new(Q(objects
				.keys()
				.cloned()
				.collect::<Vec<_>>()
				.into_iter()
				.map(|e| Ticket::new_complete(Ok(QueryResult { path: e })))))))
		} else {
			self.find(filter)
				.map_or_else(not_found, move |(obj, obj_prefix, filter)| {
					prefix.extend(obj_prefix);
					prefix.push(b'/');
					obj.query(prefix, filter)
				})
		}
	}

	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.find(path).map_or_else(not_found, |(obj, _, path)| {
			if path == b"" {
				Ticket::new_complete(Ok(obj))
			} else {
				obj.open(path)
			}
		})
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		self.find(path).map_or_else(
			|| {
				Ticket::new_complete(if path.contains(&b'/') {
					Err(Error::DoesNotExist)
				} else {
					let mut objects = self.objects.lock();
					let tbl = StreamingTable::new() as Arc<dyn Object>;
					let r = objects.insert(path.into(), Arc::downgrade(&tbl));
					assert!(r.is_none());
					Ok(tbl)
				})
			},
			|(obj, _, path)| {
				if path == b"" {
					Ticket::new_complete(Err(Error::AlreadyExists))
				} else {
					obj.create(path)
				}
			},
		)
	}
}

fn not_found<T>() -> Ticket<T> {
	Ticket::new_complete(Err(Error::DoesNotExist))
}
