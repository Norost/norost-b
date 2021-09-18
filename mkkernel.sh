#!/bin/sh

. ./env.sh

set -e

cd kernel
cargo rustc \
	--release \
	--target $RUST_TARGET_FILE \
	--features mem-max-256m \
	-- \
	-C linker=$CC \
	-C link-arg=-nostartfiles \
	-C link-arg=-Tkernel/src/arch/$ARCH/link.ld \
	-C link-arg=kernel/src/arch/$ARCH/start.s \
	-C no-redzone=yes
