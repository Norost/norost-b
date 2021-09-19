use super::common;
use core::convert::TryInto;
use core::fmt;
use crate::memory::frame;

pub fn init() {
	let root = common::get_current();

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
	}, common::IDEMPOTENT_MAP_ADDRESS, 0);
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
			if e.is_leaf() {
				writeln!(f, "  {}", t)?;
			} else if let Some(tbl) = e.as_table_mut() {
				writeln!(f, "  {}:", t)?;
				// L3
				for (g, e) in tbl.iter_mut().enumerate() {
					if e.is_leaf() {
						writeln!(f, "    {}", g)?;
					} else if let Some(tbl) = e.as_table_mut() {
						writeln!(f, "    {}:", g)?;
						// L2
						for (m, e) in tbl.iter_mut().enumerate() {
							if e.is_leaf() {
								writeln!(f, "      {}", m)?;
							} else if let Some(tbl) = e.as_table_mut() {
								writeln!(f, "      {}:", m)?;
								// L1
								for (k, e) in tbl.iter_mut().enumerate() {
									if e.is_present() {
										writeln!(f, "        {}", k)?;
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
