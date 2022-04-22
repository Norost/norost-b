#![feature(let_else)]

mod term;

use self::term::AnsiTerminal;
use std::fs;
use std::io::{self, Read, Write};
use std::iter::Filter;
use std::str;

fn main() -> std::io::Result<()> {
	std::thread::spawn(|| ()).join().unwrap();
	std::thread::yield_now();

	let mut term = self::term::AnsiTerminal::new(std::io::stdin(), std::io::stderr());
	term.set_prefix(">> ");

	let mut buf @ mut buf2 = [0; 4096];
	let mut vars = std::collections::HashMap::<Box<str>, _>::new();

	loop {
		let r = term.read(&mut buf)?;
		let mut args = buf[..r]
			.split(|c| b" \t\n\r".contains(c))
			.filter(|s| !s.is_empty());
		let Some(cmd) = args.next() else { continue; };

		let next_str =
			|term: &mut AnsiTerminal<_, _>, args: &mut Filter<_, _>| -> Result<_, io::Error> {
				let Some(s) = args.next() else {
				writeln!(term, "Missing name")?;
				return Ok(None);
			};
				let Ok(s) = str::from_utf8(s) else {
				writeln!(term, "Invalid UTF-8 for name")?;
				return Ok(None);
			};
				Ok(Some(s))
			};
		let maybe_next_str =
			|term: &mut AnsiTerminal<_, _>, args: &mut Filter<_, _>| -> Result<_, io::Error> {
				let Ok(s) = str::from_utf8(args.next().unwrap_or(b"")) else {
				writeln!(term, "Invalid UTF-8 for name")?;
				return Ok(None);
			};
				Ok(Some(s))
			};

		match cmd {
			b"help" => {
				writeln!(term, "Available commands:")?;
				writeln!(term, "  help                    Show available commands")?;
				writeln!(
					term,
					"  ls     [path]           List tables or objects in a table"
				)?;
				writeln!(
					term,
					"  open   <name> <path>    Open an object and assign the handle to a variable"
				)?;
				writeln!(
					term,
					"  create <name> <path>    Create an object and assign the handle to a variable"
				)?;
				writeln!(term, "  close  <name>           Close an object handle")?;
				writeln!(term, "  read   <name> [amount]  Read from an object")?;
				writeln!(term, "  write  <name> <data>    Write to an object")?;
				writeln!(term, "  vars                    List variables")?;
				writeln!(term, "  exit   [code]           Exit this shell")?;
			}
			b"ls" => {
				let Some(path) = maybe_next_str(&mut term, &mut args)? else { continue; };
				match fs::read_dir(path) {
					Ok(l) => {
						for e in l {
							match e {
								Ok(e) => match e.path().to_str() {
									Some(e) => writeln!(term, "{}", e),
									None => writeln!(term, "{:?}", e),
								}?,
								Err(e) => writeln!(term, "{}", e)?,
							}
						}
					}
					Err(e) => writeln!(term, "{}", e)?,
				}
			}
			b"open" => {
				let Some(name) = next_str(&mut term, &mut args)? else { continue; };
				let Some(path) = next_str(&mut term, &mut args)? else { continue; };
				match fs::File::open(path) {
					Ok(f) => {
						vars.insert(name.into(), f);
					}
					Err(e) => writeln!(term, "Failed to open \"{}\": {}", path, e)?,
				}
			}
			b"create" => {
				let Some(name) = next_str(&mut term, &mut args)? else { continue; };
				let Some(path) = next_str(&mut term, &mut args)? else { continue; };
				match fs::File::create(path) {
					Ok(f) => {
						vars.insert(name.into(), f);
					}
					Err(e) => writeln!(term, "Failed to open \"{}\": {}", path, e)?,
				}
			}
			b"close" => {
				let Some(name) = next_str(&mut term, &mut args)? else { continue; };
				if vars.remove(name).is_none() {
					writeln!(term, "No variable named \"{}\"", name)?;
				}
			}
			b"read" => {
				let Some(name) = next_str(&mut term, &mut args)? else { continue; };
				let Some(len) = maybe_next_str(&mut term, &mut args)? else { continue; };
				let Ok(len) = (if len == "" {
					Ok(usize::MAX)
				} else {
					len.parse()
				}) else {
					writeln!(term, "Length is not a valid number")?;
					continue;
				};
				let len = len.min(buf2.len());
				let Some(mut f) = vars.get(name) else {
					writeln!(term, "No variable named \"{}\"", name)?;
					continue;
				};
				match f.read(&mut buf2[..len]) {
					Ok(l) => {
						term.write(&buf2[..l])?;
						writeln!(term)?;
					}
					Err(e) => writeln!(term, "Failed to read from \"{}\"", e)?,
				}
			}
			b"write" => {
				let Some(name) = next_str(&mut term, &mut args)? else { continue; };
				// Send whatever's left
				let data = buf[..r]
					.splitn(3, |c| b" \t\n\r".contains(c))
					.filter(|s| !s.is_empty())
					.last()
					.unwrap();
				let Some(mut f) = vars.get(name) else {
					writeln!(term, "No variable named \"{}\"", name)?;
					continue;
				};
				match f.write(data) {
					Ok(l) => writeln!(term, "Wrote {} bytes", l)?,
					Err(e) => writeln!(term, "Failed to read from \"{}\"", e)?,
				}
			}
			b"vars" => {
				for v in vars.keys() {
					writeln!(term, "{}", v)?;
				}
			}
			b"exit" => {
				let Some(code) = maybe_next_str(&mut term, &mut args)? else { continue; };
				let Ok(code) = (if code == "" {
					Ok(0)
				} else {
					code.parse()
				}) else {
					writeln!(term, "Code is not a valid number")?;
					continue;
				};
				std::process::exit(code);
			}
			c => writeln!(
				term,
				"Unknown command \"{}\" - try \"help\"",
				str::from_utf8(c).unwrap_or("<invalid utf-8>")
			)?,
		};
	}
}
