use crate::workspace::Path;

pub struct Window<U> {
	/// Node path in bitmap format.
	///
	/// The lower 8 bits indicate the workspace ID. Each bit after indicates left or right in
	/// the node tree.
	///
	/// 24 bits allows up to 24 levels of windows which ought to be plenty.
	path: u32,
	pub user_data: U,
}

impl<U> Window<U> {
	pub fn new(workspace: u8, path: Path, user_data: U) -> Self {
		let mut s = Self { path: 0, user_data };
		s.set_path(workspace, path);
		s
	}

	pub fn path(&self) -> (u8, PathIter) {
		(
			self.path as u8,
			PathIter { count: 24, path: self.path >> 8 },
		)
	}

	pub fn set_path(&mut self, workspace: u8, path: Path) {
		assert!(path.depth <= 24, "deeper than 24 levels");
		self.path = u32::from(workspace) | (path.directions << 8)
	}
}

pub struct PathIter {
	count: u8,
	path: u32,
}

impl PathIter {
	#[inline(always)]
	pub fn new(depth: u8, directions: u32) -> Self {
		Self { count: depth, path: directions }
	}

	/// Create a path iterator that goes to the right bottom for up to 24 levels.
	#[inline(always)]
	pub fn right_bottom() -> Self {
		Self::new(24, 0xffffff)
	}
}

impl Default for PathIter {
	fn default() -> Self {
		Self { count: 24, path: 0 }
	}
}

impl Iterator for PathIter {
	type Item = bool;

	fn next(&mut self) -> Option<Self::Item> {
		self.count.checked_sub(1).map(|c| {
			self.count = c;
			let v = self.path & 1 != 0;
			self.path >>= 1;
			v
		})
	}
}

#[derive(Default)]
pub struct GlobalWindowParams {
	pub border_width: u32,
}
