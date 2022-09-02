use {super::common, core::fmt};

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
