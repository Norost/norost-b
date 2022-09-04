use {
	crate::{
		boot,
		memory::{
			frame::{PPNBox, PageFrameIter, PPN},
			r#virtual::RWX,
			Page,
		},
		object_table::{Error, MemoryObject, Object, PageFlags, Root, Ticket, TinySlice},
	},
	alloc::{boxed::Box, sync::Arc},
};

static mut BASE: PPNBox = 0;
static mut INFO: FramebufferInfo = FramebufferInfo {
	pitch: 0,
	width: 0,
	height: 0,
	bpp: 0,
	r_pos: 0,
	r_mask: 0,
	g_pos: 0,
	g_mask: 0,
	b_pos: 0,
	b_mask: 0,
};

#[repr(C)]
struct FramebufferInfo {
	pitch: u32,
	width: u16,
	height: u16,
	bpp: u8,
	r_pos: u8,
	r_mask: u8,
	g_pos: u8,
	g_mask: u8,
	b_pos: u8,
	b_mask: u8,
}

struct Framebuffer;

impl Object for Framebuffer {
	fn get_meta(self: Arc<Self>, property: &TinySlice<u8>) -> Ticket<Box<[u8]>> {
		Ticket::new_complete(match &**property {
			// SAFETY: INFO is not modified after init.
			// There are no holes in FramebufferInfo.
			b"bin/info" => {
				Ok(unsafe { &*(&INFO as *const _ as *const [u8; 4 + 2 * 2 + 7]) }[..].into())
			}
			_ => Err(Error::DoesNotExist),
		})
	}

	fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
		Some(self)
	}
}

unsafe impl MemoryObject for Framebuffer {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
		// SAFETY: BASE is not modified after init.
		for p in (PageFrameIter { base: unsafe { PPN(BASE) }, count: self.physical_pages_len() }) {
			if !f(&[p]) {
				break;
			}
		}
	}

	fn physical_pages_len(&self) -> usize {
		unsafe {
			// SAFETY: INFO is not modified after init.
			Page::min_pages_for_bytes((INFO.pitch as usize + 1) * (INFO.height as usize + 1))
		}
	}

	fn page_flags(&self) -> (PageFlags, RWX) {
		(*PageFlags::default().set_write_combining(), RWX::RW)
	}
}

pub unsafe fn init(boot: &boot::Info) {
	// SAFETY: Only we are modifying BASE and INFO
	unsafe {
		BASE = (boot.framebuffer.base / Page::SIZE as u64)
			.try_into()
			.unwrap();
		INFO.pitch = boot.framebuffer.pitch;
		INFO.width = boot.framebuffer.width;
		INFO.height = boot.framebuffer.height;
		INFO.bpp = boot.framebuffer.bpp;
		INFO.r_pos = boot.framebuffer.r_pos;
		INFO.r_mask = boot.framebuffer.r_mask;
		INFO.g_pos = boot.framebuffer.g_pos;
		INFO.g_mask = boot.framebuffer.g_mask;
		INFO.b_pos = boot.framebuffer.b_pos;
		INFO.b_mask = boot.framebuffer.b_mask;
	}
}

pub fn post_init(root: &Root) {
	// SAFETY: BASE is not modified after init.
	if unsafe { BASE } != 0 {
		let o = Arc::new(Framebuffer) as Arc<dyn Object>;
		root.add(*b"framebuffer", Arc::downgrade(&o));
		let _ = Arc::into_raw(o);
	}
}
