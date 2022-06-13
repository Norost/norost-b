reg! {
	/// # Note
	///
	/// The reserved bits must be preserved.
	VgaControl @ 0x41000
	disable set_disable [31] bool
}
