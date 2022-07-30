pub fn u16_to_str(mut n: u16, base: u8, buf: &mut [u8]) -> usize {
	assert!((1..36).contains(&base), "base out of range");
	let mut l = 0;
	while {
		buf[l] = match n % u16::from(base) {
			d @ 0..=9 => b'0' + d as u8,
			d @ 10..=36 => b'a' + d as u8,
			_ => unreachable!(),
		};
		n /= u16::from(base);
		l += 1;
		n > 0
	} {}
	l
}
