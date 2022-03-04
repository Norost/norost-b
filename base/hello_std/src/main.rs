#![feature(path_try_exists)]

fn main() {
	dbg!(dbg!(std::fs::read_dir("")).unwrap().collect::<Vec<_>>());
	dbg!(dbg!(std::fs::read_dir("pci/")).unwrap().collect::<Vec<_>>());

	dbg!(std::fs::File::open("pci/name:0:00.0,vendor-id:8086,device-id:29c0/0").unwrap());
	dbg!(std::fs::File::open("pci/name:0:00.0,vendor-id:8086,device-id:29c0").unwrap());

	dbg!(std::fs::try_exists("pci/vendor-id:8086,device-id:29c0").unwrap());
	dbg!(std::fs::try_exists("pci/vendor-id:ffff,device-id:29c0").unwrap());

	use std::io::{BufRead, BufReader, Write};
	let mut f = std::fs::File::open("uart//0").unwrap();
	writeln!(
		f,
		"I write this to an opened file and now I will read a single line"
	)
	.unwrap();
	let mut s = String::new();
	BufReader::new(f).read_line(&mut s).unwrap();
	println!("The line is {:?}", s);
}
