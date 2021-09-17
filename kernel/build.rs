fn main() {
	println!("cargo:rerun-if-changed=build.rs");
	println!("cargo:rerun-if-changed=src/arch/amd64/start.s");
	println!("cargo:rerun-if-changed=src/arch/amd64/link.ld");
}
