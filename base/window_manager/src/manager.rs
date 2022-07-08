use crate::{
	math::{Rect, Size, Vector},
	window::{GlobalWindowParams, PathIter, Window},
	workspace::{Direction, NewWorkspaceError, Workspace},
};
use alloc::boxed::Box;
use driver_utils::{Arena, Handle};

pub struct Manager<U> {
	windows: Arena<Window<U>>,
	workspaces: Box<[Workspace]>,
	current_workspace: u8,
	global_window_params: GlobalWindowParams,
}

impl<U> Manager<U> {
	pub fn new(global_window_params: GlobalWindowParams) -> Result<Self, NewManagerError> {
		let ws = Workspace::new().map_err(NewManagerError::NewWorkspace)?;
		Ok(Self {
			windows: Arena::new(),
			workspaces: [ws].into(),
			current_workspace: 0,
			global_window_params,
		})
	}

	pub fn new_window(&mut self, total_size: Size, user_data: U) -> Result<Handle, ()> {
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
			Window::new(self.current_workspace, p, user_data)
		});
		update.map(|(handle, path)| self.windows[handle].set_path(self.current_workspace, path));
		Ok(res)
	}

	pub fn destroy_window(&mut self, handle: Handle) -> Result<(), ()> {
		let w = self.windows.remove(handle).ok_or(())?;
		let (ws, path) = w.path();
		self.workspaces[usize::from(ws)].remove_leaf(path);
		Ok(())
	}

	pub fn window_rect(&self, handle: Handle, total_size: Size) -> Option<Rect> {
		let window = self.windows.get(handle)?;
		let (ws, path) = window.path();
		(self.current_workspace == ws)
			.then(|| self.workspaces[usize::from(ws)].calculate_rect(path, total_size))
			.flatten()
			.map(|rect| {
				let d = Vector::ONE * self.global_window_params.border_width;
				Rect::from_points(rect.low() + d, rect.high() - d)
			})
	}

	pub fn window(&self, handle: Handle) -> Option<&Window<U>> {
		self.windows.get(handle)
	}

	pub fn window_mut(&mut self, handle: Handle) -> Option<&mut Window<U>> {
		self.windows.get_mut(handle)
	}

	pub fn windows(&self) -> impl Iterator<Item = (Handle, &Window<U>)> + '_ {
		self.windows.iter()
	}

	pub fn windows_mut(&mut self) -> impl Iterator<Item = (Handle, &mut Window<U>)> + '_ {
		self.windows.iter_mut()
	}
}

#[derive(Debug)]
pub enum NewManagerError {
	NoFileRoot,
	NewWorkspace(NewWorkspaceError),
}
