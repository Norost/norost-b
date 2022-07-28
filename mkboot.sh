#!/bin/sh

set -e
set -x

. ./env.sh

# We don't enable interrupts in the bootloader, so using redzone is fine.

cd boot/$ARCH
cargo rustc "$@" -- \
	-C llvm-args=-align-all-blocks=1 \
	-C linker=$CC \
	-C link-arg=-m32 \
	-C link-arg=-nostartfiles \
	-C link-arg=-Tboot/$ARCH/link.ld \
	-C link-arg=boot/$ARCH/src/start.s \
	-C no-redzone=no \
	-Z unstable-options \
	-C split-debuginfo=unpacked
