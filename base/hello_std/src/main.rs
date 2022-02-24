fn main() {
	dbg!(dbg!(std::fs::read_dir("")).unwrap().collect::<Vec<_>>());
}
