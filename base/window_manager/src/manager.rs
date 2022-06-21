use crate::{
	math::{Rect, Size, Vector},
	window::{GlobalWindowParams, PathIter, Window},
	workspace::{Direction, NewWorkspaceError, Workspace},
};
use alloc::boxed::Box;
use driver_utils::{Arena, Handle};

pub struct Manager {
	table: rt::Object,
	windows: Arena<Window>,
	workspaces: Box<[Workspace]>,
	current_workspace: u8,
	global_window_params: GlobalWindowParams,
}

impl Manager {
	pub fn new(global_window_params: GlobalWindowParams) -> Result<Self, NewManagerError> {
		let ws = Workspace::new().map_err(NewManagerError::NewWorkspace)?;
		let table = rt::io::file_root()
			.ok_or(NewManagerError::NoFileRoot)?
			.create(b"window_manager")
			.map_err(NewManagerError::Io)?;
		Ok(Self {
			table,
			windows: Arena::new(),
			workspaces: [ws].into(),
			current_workspace: 0,
			global_window_params,
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

	pub fn window_rect(&self, handle: Handle, total_size: Size) -> Option<Rect> {
		let window = self.windows.get(handle)?;
		let (ws, path) = window.path();
		(self.current_workspace == ws)
			.then(|| self.workspaces[usize::from(ws)].calculate_rect(path, total_size))
			.flatten()
			.map(|rect| {
				let d = Vector::ONE * self.global_window_params.border_width;
				Rect::new(rect.low() + d, rect.high() - d)
			})
	}

	#[inline(always)]
	pub fn window_handles(&self) -> impl Iterator<Item = Handle> + '_ {
		self.windows.iter().map(|(h, _)| h)
	}
}

#[derive(Debug)]
pub enum NewManagerError {
	NoFileRoot,
	Io(rt::io::Error),
	NewWorkspace(NewWorkspaceError),
}
