CC=gcc
RUST_TARGET=../x86_64-unknown-norostb.json
CARGO_OPT=
ARCH=amd64

cd kernel
cargo rustc \
	--release \
	--target $RUST_TARGET \
	$CARGO_OPT \
	-- \
	-C linker=$CC \
	-C link-arg=-nostartfiles \
	-C link-arg=-Tkernel/src/arch/$ARCH/link.ld \
	-C link-arg=kernel/src/arch/$ARCH/start.s \
	-C no-redzone=yes
