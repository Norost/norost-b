#!/bin/sh

. ./env.sh

set -e

cd kernel
cargo rustc --release -- \
	-C link-arg=-Tkernel/src/arch/$ARCH/link.ld \
	-C link-arg=kernel/src/arch/$ARCH/start.s \
	-C linker=$CC \
	-C link-arg=-nostartfiles \
	-C no-redzone=yes
