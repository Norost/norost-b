fn main() {
	let mut s = String::with_capacity(128);
	println!("Hello, world!");
	loop {
		print!("> ");
		use std::io::Write;
		std::io::stdout().flush().unwrap();
		std::io::stdin().read_line(&mut s).unwrap();
		println!("You wrote: {:?}", s);
		s.clear()
	}
}
