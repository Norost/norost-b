pub use rt::exit;

pub struct Stdio(StdioTy);

enum StdioTy {
	Piped,
	Inherit,
	Null,
}

impl Stdio {
	pub fn piped() -> Self {
		Self(StdioTy::Piped)
	}

	pub fn inherit() -> Self {
		Self(StdioTy::Inherit)
	}

	pub fn null() -> Self {
		Self(StdioTy::Null)
	}
}
