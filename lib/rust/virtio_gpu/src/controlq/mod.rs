//! # 2D commands

pub mod resource;

mod display_info;
mod edid;
mod rect;
mod set_scanout;
mod transfer_to_host_2d;

pub use {
	display_info::*, edid::*, rect::Rect, set_scanout::SetScanout,
	transfer_to_host_2d::TransferToHost2D,
};

use {
	crate::ControlHeader,
	endian::{u32le, u64le},
};
