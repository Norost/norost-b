use super::{Error, Object, Query, QueryResult, StreamingTable, Ticket};
use crate::sync::Mutex;
use alloc::{
	boxed::Box,
	collections::BTreeMap,
	sync::{Arc, Weak},
	vec::Vec,
};

/// The root object. This object is passed to all other processes on the system.
pub struct Root;

impl Root {
	/// Add a new object to the root.
	pub fn add(name: impl Into<Box<[u8]>>, object: Weak<dyn Object>) {
		OBJECTS.lock().insert(name.into(), object);
	}
}

/// All objects located at the root.
static OBJECTS: Mutex<BTreeMap<Box<[u8]>, Weak<dyn Object>>> = Mutex::new(BTreeMap::new());

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
			Ticket::new_complete(Ok(Box::new(Q(OBJECTS
				.lock()
				.keys()
				.cloned()
				.collect::<Vec<_>>()
				.into_iter()
				.map(|e| Ticket::new_complete(Ok(QueryResult { path: e })))))))
		} else {
			find(filter).map_or_else(not_found, move |(obj, obj_prefix, filter)| {
				prefix.extend(obj_prefix);
				prefix.push(b'/');
				obj.query(prefix, filter)
			})
		}
	}

	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		find(path).map_or_else(not_found, |(obj, _, path)| {
			if path == b"" {
				Ticket::new_complete(Ok(obj))
			} else {
				obj.open(path)
			}
		})
	}

	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		find(path).map_or_else(
			|| {
				let mut objects = OBJECTS.lock();
				let tbl = StreamingTable::new() as Arc<dyn Object>;
				let r = objects.insert(path.into(), Arc::downgrade(&tbl));
				assert!(r.is_none());
				Ticket::new_complete(Ok(tbl))
			},
			|(obj, _, path)| {
				if path == b"" {
					Ticket::new_complete(Err(Error::new(1, "object already exists".into())))
				} else {
					obj.create(path)
				}
			},
		)
	}
}

fn find(path: &[u8]) -> Option<(Arc<dyn Object>, &[u8], &[u8])> {
	let (object, rest) = path
		.iter()
		.position(|c| *c == b'/')
		.map_or((path, &b""[..]), |i| (&path[..i], &path[i + 1..]));
	let mut objects = OBJECTS.lock();
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

fn not_found<T>() -> Ticket<T> {
	Ticket::new_complete(Err(Error::new(1, "object not found".into())))
}
