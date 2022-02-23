#!/bin/sh

. ./env.sh

set -e
set -x

cd boot/$ARCH
cargo rustc --release -- \
	-C llvm-args=-align-all-blocks=1 \
	-C linker=$CC \
	-C link-arg=-m32 \
	-C link-arg=-nostartfiles \
	-C link-arg=-Tboot/$ARCH/link.ld \
	-C link-arg=boot/$ARCH/src/start.s \
	-C no-redzone=yes
