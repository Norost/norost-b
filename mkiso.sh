#!/bin/sh

. ./env.sh

./mkkernel.sh || exit $?
./mkboot.sh || exit $?

set -e

mkdir -p isodir/boot/grub isodir/drivers
cp target/$RUST_TARGET/release/nora isodir/boot/nora
cp target/$RUST_TARGET_32/release/noraboot isodir/boot/noraboot
cp boot/$ARCH/grub/grub.cfg isodir/boot/grub/grub.cfg
(cd drivers/hello_world && ./build.sh)
cp drivers/hello_world/hello isodir/drivers/hello_world
grub-mkrescue -o norost.iso isodir
