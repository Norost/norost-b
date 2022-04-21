use super::common;
use crate::memory::frame;
use core::fmt;

/// # Safety
///
/// This function may only be called once.
pub(super) unsafe fn init() {
	let root = common::get_current();

	// Add tables for all of the higher half memory.
	//
	// This is so these global tables can be reused for new page tables without modifying
	// existing tables.
	let mut i = 256
		+ root[256..256 + 128]
			.iter()
			.filter(|e| e.is_present())
			.count();

	frame::allocate(
		256 + 128 - i,
		|frame| {
			assert_eq!(frame.p2size, 0); // shouldn't happen on amd64 platforms with count < 512
			while root[i].is_present() {
				i += 1;
				assert!(i < 256 + 128);
			}
			root[i].new_table(frame, false);
			i += 1;
		},
		common::IDENTITY_MAP_ADDRESS,
		0,
	)
	.unwrap();
}

pub struct DumpCurrent;

impl fmt::Debug for DumpCurrent {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		writeln!(f, "PML4 (CR3)")?;
		let root = common::get_current();
		// TODO make this recursive. get_entry_mut should be suitable.
		// L4
		for (t, e) in root.iter_mut().enumerate() {
			if let Some(tbl) = e.as_table_mut() {
				writeln!(f, "{:>3}:", t)?;
				// L3
				for (g, e) in tbl.iter_mut().enumerate() {
					if e.is_leaf() {
						writeln!(f, " 1G {:>3}", g)?;
					} else if let Some(tbl) = e.as_table_mut() {
						writeln!(f, "PDP {:>3}:", g)?;
						// L2
						for (m, e) in tbl.iter_mut().enumerate() {
							if e.is_leaf() {
								writeln!(f, "    2M {:>3}", m)?;
							} else if let Some(tbl) = e.as_table_mut() {
								writeln!(f, "    PD {:>3}:", m)?;
								// L1
								for (k, e) in tbl.iter_mut().enumerate() {
									if e.is_present() {
										writeln!(f, "         4K {:>3}", k)?;
									}
								}
							}
						}
					}
				}
			}
		}
		Ok(())
	}
}
