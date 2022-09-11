use {
	crate::{
		math::{Point2, Rect, Size, Vec2},
		window::{PathIter, Window},
		workspace::{NewWorkspaceError, Workspace},
	},
	core::cell::Cell,
	driver_utils::{Arena, Handle},
	std::boxed::Box,
};

macro_rules! mgr {
	($self:expr, current_workspace) => {
		$self.workspaces[$self.current_workspace()]
	};
}

pub struct Manager {
	pub windows: Arena<Window>,
	pub workspaces: Box<[Workspace]>,
	current_workspace: u8,
	focused_window: Cell<Handle>,
}

impl Manager {
	pub fn new() -> Result<Self, NewManagerError> {
		let ws = Workspace::new().map_err(NewManagerError::NewWorkspace)?;
		Ok(Self {
			windows: Arena::new(),
			workspaces: [ws].into(),
			current_workspace: 0,
			focused_window: Handle::MAX.into(),
		})
	}

	pub fn new_window(&mut self, total_size: Size) -> Result<Handle, ()> {
		let mut update = None;
		let res = self.windows.insert_with(|handle| {
			let p;
			(p, update) = self.workspaces[usize::from(self.current_workspace)]
				.split_leaf(
					PathIter::right_bottom(),
					handle,
					None,
					Default::default(),
					total_size,
				)
				.unwrap_or_else(|e| todo!("{:?}", e));
			Window::new(self.current_workspace, p)
		});
		update.map(|(handle, path)| self.windows[handle].set_path(self.current_workspace, path));
		Ok(res)
	}

	pub fn destroy_window(&mut self, handle: Handle) -> Result<Window, ()> {
		let w = self.windows.remove(handle).ok_or(())?;
		let (ws, path) = w.path();
		let path = self.workspaces[usize::from(ws)].remove_leaf(path).unwrap();
		let len = path.depth.into();
		self.workspaces[usize::from(ws)].apply_with_prefix(path.into_iter(), |h| {
			self.windows[h].move_up(len);
		});
		Ok(w)
	}

	pub fn window_rect(&self, handle: Handle, total_size: Size) -> Option<Rect> {
		let window = self.windows.get(handle)?;
		let (ws, path) = window.path();
		(self.current_workspace == ws)
			.then(|| self.workspaces[usize::from(ws)].calculate_rect(path, total_size))
			.flatten()
	}

	pub fn window_at(&self, position: Point2, total_size: Size) -> Option<(Handle, Rect)> {
		self.workspaces[self.current_workspace()].window_at(position, total_size)
	}

	pub fn window(&self, handle: Handle) -> Option<&Window> {
		self.windows.get(handle)
	}

	pub fn window_mut(&mut self, handle: Handle) -> Option<&mut Window> {
		self.windows.get_mut(handle)
	}

	pub fn focused_window(&self) -> Option<Handle> {
		let mut it = self.workspaces[self.current_workspace()].windows();
		let h = it.next()?;
		let fw = self.focused_window.get();
		if h != fw && !it.find(|h| h == &fw).is_some() {
			self.focused_window.set(h);
		}
		Some(self.focused_window.get())
	}

	pub fn set_focused_window(&mut self, handle: Handle) {
		self.focused_window.set(handle);
	}

	pub fn current_workspace(&self) -> usize {
		self.current_workspace.into()
	}
}

#[derive(Debug)]
pub enum NewManagerError {
	NewWorkspace(NewWorkspaceError),
}
