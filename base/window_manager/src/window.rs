use {
	crate::workspace::Path,
	core::fmt::{self, Write},
};

pub struct Window<U> {
	/// Workspace containing this window.
	workspace: u8,
	/// Node path in bitmap format.
	path: u32,
	pub user_data: U,
}

impl<U> Window<U> {
	pub fn new(workspace: u8, path: Path, user_data: U) -> Self {
		let mut s = Self { workspace, path: 0, user_data };
		s.set_path(workspace, path);
		s
	}

	pub fn path(&self) -> (u8, PathIter) {
		(self.workspace, PathIter { count: 32, path: self.path })
	}

	pub fn set_path(&mut self, workspace: u8, path: Path) {
		assert!(path.depth <= 32, "deeper than 32 levels");
		self.workspace = workspace;
		self.path = path.directions;
	}

	/// Move this window one layer up the tree.
	pub fn move_up(&mut self, from: usize) {
		let mask = (1 << from) - 1;
		self.path = self.path & mask | self.path >> 1 & !mask;
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

impl fmt::Debug for PathIter {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let mut p = self.path;
		f.write_char('<')?;
		for _ in 0..self.count {
			f.write_char(['_', '#'][(p & 1) as usize])?;
			p >>= 1;
		}
		f.write_char('>')
	}
}

#[derive(Default)]
pub struct GlobalWindowParams {
	pub border_width: u32,
}
