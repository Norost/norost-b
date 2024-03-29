use {
	crate::{
		math::{Point2, Ratio, Rect, Size},
		window::PathIter,
	},
	core::{fmt, mem},
	driver_utils::{Arena, Handle},
};

pub struct Workspace {
	nodes: Arena<Node>,
	root: Handle,
}

// TODO consider making it doubly linked to avoid excessive use of Paths
enum Node {
	Parent { left: Handle, right: Handle, vertical: bool, ratio: Ratio },
	Leaf { window: Handle },
}

impl Workspace {
	pub fn new() -> Result<Self, NewWorkspaceError> {
		Ok(Self { nodes: Arena::new(), root: 0 })
	}

	/// Split the first leaf node along the given path. It returns the path of the new leaf
	/// as well as the handle of the leaf that was split along with its new path, if there
	/// was any.
	///
	/// If direction is [`None`], either a `Right` or `Down` direction is chosen such that
	/// the aspect ratio is as close to 1 as possible.
	pub fn split_leaf(
		&mut self,
		mut path: PathIter,
		window: Handle,
		direction: Option<Direction>,
		ratio: Ratio,
		mut size: Size,
	) -> Result<(Path, Option<(Handle, Path)>), SplitLeafError> {
		let mut directions = 0;
		let mut cur = self.root;
		if !self.nodes.is_empty() {
			for depth in 1..24 {
				match &self.nodes[cur] {
					Node::Parent { left, right, vertical, ratio } => {
						let d = path.next().expect("path does not lead to a leaf");
						directions |= u32::from(d) << (depth - 1);
						let v = if *vertical { &mut size.y } else { &mut size.x };
						let (l, r) = ratio.partition(*v);
						(cur, *v) = if d { (*right, r) } else { (*left, l) };
					}
					Node::Leaf { window: w } => {
						let w = *w;
						let mut right = self.nodes.insert(Node::Leaf { window: w });
						let mut left = self.nodes.insert(Node::Leaf { window });
						let direction = match direction {
							Some(d) => d,
							None => {
								if size.x < size.y {
									Direction::Down
								} else {
									Direction::Right
								}
							}
						};
						let d = match direction {
							Direction::Right | Direction::Down => {
								mem::swap(&mut left, &mut right);
								true
							}
							Direction::Left | Direction::Up => false,
						};
						let vertical = match direction {
							Direction::Right | Direction::Left => false,
							Direction::Up | Direction::Down => true,
						};
						self.nodes[cur] = Node::Parent { left, right, vertical, ratio };
						directions |= u32::from(d) << (depth - 1);
						return Ok((
							Path { depth, directions },
							Some((
								w,
								Path { depth, directions: directions ^ (1 << (depth - 1)) },
							)),
						));
					}
				}
			}
			Err(SplitLeafError::TooDeep)
		} else {
			self.root = self.nodes.insert(Node::Leaf { window });
			Ok((Path { depth: 0, directions: 0 }, None))
		}
	}

	/// Remove a leaf, replacing its parent with its sibling.
	///
	/// Returns the path to the sibling node, if any.
	///
	/// # Panics
	///
	/// The path does not lead to a leaf.
	pub fn remove_leaf(&mut self, mut rem_path: PathIter) -> Option<Path> {
		let mut cur = self.root;
		let mut prev = None;
		let mut path = Path { depth: 0, directions: 0 };
		loop {
			match &self.nodes[cur] {
				Node::Parent { left, right, .. } => {
					let d = rem_path.next().expect("path does not lead to leaf");
					let s = *if d { left } else { right };
					prev = Some((cur, s, path));
					cur = *if d { right } else { left };
					path.directions |= u32::from(d) << path.depth;
					path.depth += 1;
				}
				Node::Leaf { window } => {
					let w = *window;
					self.nodes.remove(cur).unwrap();
					return if let Some((parent, sibling, path)) = prev {
						self.nodes[parent] = self.nodes.remove(sibling).unwrap();
						Some(path)
					} else {
						None
					};
				}
			}
		}
	}

	/// Call the closure with the handles in all leaves with the given path as prefix.
	pub fn apply_with_prefix(&self, mut prefix: PathIter, mut cb: impl FnMut(Handle)) {
		let mut cur = self.root;
		for d in prefix {
			match &self.nodes[cur] {
				Node::Parent { left, right, .. } => {
					let s = *if d { left } else { right };
					cur = *if d { right } else { left };
				}
				Node::Leaf { window } => return,
			}
		}
		fn f(slf: &Workspace, cur: Handle, cb: &mut impl FnMut(Handle)) {
			match &slf.nodes[cur] {
				Node::Parent { left, right, .. } => {
					f(slf, *left, cb);
					f(slf, *right, cb);
				}
				Node::Leaf { window } => cb(*window),
			}
		}
		f(self, cur, &mut cb)
	}

	/// Calculate the [`Rect`] a leaf occupies.
	///
	/// # Panics
	///
	/// The path does not lead to a valid node.
	pub fn calculate_rect(&self, mut path: PathIter, size: Size) -> Option<Rect> {
		Some(
			self.recurse(size, |l, r| {
				path.next().expect("path does not lead to a leaf")
			})?
			.1,
		)
	}

	/// Find the window at the given position.
	pub fn window_at(&self, position: Point2, size: Size) -> Option<(Handle, Rect)> {
		self.recurse(size, |_, r| r.contains(position))
	}

	/// Return an iterator over all window handles held by this workspace.
	pub fn windows(&self) -> impl Iterator<Item = Handle> + '_ {
		self.nodes.iter().flat_map(|(_, n)| match n {
			Node::Parent { .. } => None,
			Node::Leaf { window } => Some(*window),
		})
	}

	/// Whether this workspace contains any windows at all.
	pub fn is_empty(&self) -> bool {
		self.nodes.is_empty()
	}

	/// Recurse in the tree, going left (`false`) or right (`true`) based on the given predicate.
	///
	/// Returns the handle of the window if any were found as well as the calculated rect.
	fn recurse<F>(&self, size: Size, mut pred: F) -> Option<(Handle, Rect)>
	where
		F: FnMut(&Rect, &Rect) -> bool,
	{
		let mut cur = self.nodes.get(self.root)?; // Having no root node is valid
		let mut rect = Rect::from_size(Point2::ORIGIN, size);
		loop {
			match cur {
				Node::Parent { left, right, ratio, vertical } => {
					let (rect_l, rect_r) = if *vertical {
						let mid = ratio.partition_range(rect.y());
						let (y_l, y_r) = (rect.low().y..=mid, mid + 1..=rect.high().y);
						(
							Rect::from_ranges(rect.x(), y_l),
							Rect::from_ranges(rect.x(), y_r),
						)
					} else {
						let mid = ratio.partition_range(rect.x());
						let (x_l, x_r) = (rect.low().x..=mid, mid + 1..=rect.high().x);
						(
							Rect::from_ranges(x_l, rect.y()),
							Rect::from_ranges(x_r, rect.y()),
						)
					};
					cur = &self.nodes[*if pred(&rect_l, &rect_r) {
						rect = rect_r;
						right
					} else {
						rect = rect_l;
						left
					}];
				}
				Node::Leaf { window } => return Some((*window, rect)),
			}
		}
	}
}

#[derive(Clone, Copy)]
pub struct Path {
	pub depth: u8,
	pub directions: u32,
}

impl IntoIterator for Path {
	type IntoIter = PathIter;
	type Item = <PathIter as Iterator>::Item;

	fn into_iter(self) -> PathIter {
		PathIter::new(self.depth, self.directions)
	}
}

impl fmt::Debug for Path {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{}:{:^width$}",
			self.depth,
			self.directions,
			width = self.depth.into()
		)
	}
}

#[derive(Clone, Copy)]
pub enum Direction {
	#[allow(dead_code)]
	Left,
	#[allow(dead_code)]
	Up,
	Right,
	Down,
}

#[derive(Debug)]
pub enum NewWorkspaceError {}

#[derive(Debug)]
pub enum SplitLeafError {
	TooDeep,
}

#[cfg(test)]
mod test {
	use super::*;

	fn ws() -> Workspace {
		Workspace::new().unwrap()
	}

	fn split_leaf_dir(ws: &mut Workspace, dir: Direction) -> Path {
		ws.split_leaf(PathIter::default(), 0, Some(dir), Ratio::HALF, Size::ZERO)
			.unwrap()
			.0
	}

	#[test]
	fn ratio_root_only() {
		let mut ws = ws();
		let path = split_leaf_dir(&mut ws, Direction::Up);
		let size = Size::new(100, 100);
		assert_eq!(
			ws.calculate_rect(path.into_iter(), size),
			Some(Rect::from_size(Point2::ORIGIN, size)),
		);
	}

	#[test]
	fn ratio_half_left() {
		let mut ws = ws();
		split_leaf_dir(&mut ws, Direction::Up);
		let path = split_leaf_dir(&mut ws, Direction::Left);
		let size = Size::new(100, 100);
		assert_eq!(
			ws.calculate_rect(path.into_iter(), size),
			Some(Rect::from_size(
				Point2::ORIGIN,
				Size::new(size.x / 2, size.y)
			)),
		);
	}

	#[test]
	fn ratio_half_right() {
		let mut ws = ws();
		split_leaf_dir(&mut ws, Direction::Up);
		let path = split_leaf_dir(&mut ws, Direction::Right);
		let size = Size::new(100, 100);
		assert_eq!(
			ws.calculate_rect(path.into_iter(), size),
			Some(Rect::from_size(
				Point2::new(50, 0),
				Size::new(size.x / 2, size.y)
			)),
		);
	}

	#[test]
	fn ratio_half_up() {
		let mut ws = ws();
		split_leaf_dir(&mut ws, Direction::Up);
		let path = split_leaf_dir(&mut ws, Direction::Up);
		let size = Size::new(100, 100);
		assert_eq!(
			ws.calculate_rect(path.into_iter(), size),
			Some(Rect::from_size(
				Point2::ORIGIN,
				Size::new(size.x, size.y / 2)
			)),
		);
	}

	#[test]
	fn split_leaf_path_root_only() {
		let mut ws = ws();
		let path = split_leaf_dir(&mut ws, Direction::Up);
		assert_eq!(path.depth, 0);
	}

	#[test]
	fn split_leaf_path_half_left() {
		let mut ws = ws();
		split_leaf_dir(&mut ws, Direction::Up);
		let path = split_leaf_dir(&mut ws, Direction::Left);
		assert_eq!(path.depth, 1);
		assert_eq!(path.directions, 0);
	}

	#[test]
	fn split_leaf_path_half_right() {
		let mut ws = ws();
		split_leaf_dir(&mut ws, Direction::Up);
		let path = split_leaf_dir(&mut ws, Direction::Right);
		assert_eq!(path.depth, 1);
		assert_eq!(path.directions, 1);
	}
}
