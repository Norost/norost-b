use super::common;
use core::convert::{TryFrom, TryInto};
use core::fmt;
use crate::memory::frame;

pub fn init() {
	let root = common::get_current();

	// Unmap the one identity mapped page.
	let mut virt = 0;
	while virt & (1 << 47) == 0 {
		match common::get_entry_mut(root, virt, 0, 3) {
			Ok(e) => match e.clear() {
				Some(_) => {
					// Free the pages
					unsafe {
						// PT, PD, PDP
						for l in 1..=3 {
							let e = common::get_entry_mut(root, virt, l, 3 - l);
							let ppn = e.unwrap_or_else(|_| unreachable!()).clear().unwrap();
							frame::deallocate(1, || frame::PageFrame::from_raw(ppn, 0));
						}
					}
					break;
				},
				None => virt += 0x1000,
			}
			Err((_, d)) => virt += 12 << (u64::from(d) * 9),
		}
	}

	debug!("{:#?}", DumpCurrent);
	loop {}

	// Add tables for all of the higher half memory.
	//
	// This is so these global tables can be reused for new page tables without modifying
	// existing tables.
	let mut i = 256;
	frame::allocate(128, |frame| {
		assert_eq!(frame.p2size, 0); // shouldn't happen on amd64 platforms with count < 512
		i += 1;
		// SAFETY: physical identity mappings are still active
		unsafe { root[i].new_table(frame) };
	}, common::IDENTITY_MAP_ADDRESS, 0);
}

pub struct DumpCurrent;

impl fmt::Debug for DumpCurrent {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		use core::fmt::Write;
		let mut d = writeln!(f, "PML4 (CR3)")?;
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
