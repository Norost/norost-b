fn main() {
	loop {
		println!("Hello, world!");
		std::thread::sleep(std::time::Duration::new(1, 500 * 1_000_000));
	}
}
