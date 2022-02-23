use std::env;

fn main() {
	match env::var("TARGET") {
		Err(env::VarError::NotPresent) => panic!("TARGET is not set"),
		Err(env::VarError::NotUnicode(_)) => panic!("invalid target triple"),
		Ok(t) if t.ends_with("-norostbkernel") => (),
		Ok(t) => panic!(
			"unsupported target '{}'. Only *-norostbkernel targets are supported",
			t
		),
	}

	println!("cargo:rerun-if-changed=build.rs");
	println!("cargo:rerun-if-changed=src/arch/amd64/start.s");
	println!("cargo:rerun-if-changed=src/arch/amd64/link.ld");
}
