#!/bin/sh

. ./env.sh

./mkkernel.sh || exit $?
./mkboot.sh || exit $?

set -e

TARGET_BOOT=i686-unknown-none-norostbkernel
TARGET_KERNEL=x86_64-unknown-none-norostbkernel
TARGET_USER=x86_64-unknown-norostb

mkdir -p isodir/boot/grub isodir/drivers
cp target/$TARGET_KERNEL/release/nora isodir/boot/nora
cp target/$TARGET_BOOT/release/noraboot isodir/boot/noraboot
cp boot/$ARCH/grub/grub.cfg isodir/boot/grub/grub.cfg
cp init isodir/init

install () {
	(cd $1/$2 && cargo build --release --target $TARGET_USER)
	cp target/$TARGET_USER/release/$3 isodir/drivers/$2
}

install drivers fs_fat             driver_fs_fat
install drivers virtio_block       driver_virtio_block
install drivers virtio_net         driver_virtio_net
install base    init               init
install base    jail               jail
install base    minish             minish
install base    static_http_server static_http_server

# Note: make sure grub-pc-bin is installed! Otherwise QEMU may hang on
# "Booting from disk" or return error code 0009
grub-mkrescue -o norost.iso isodir \
	--locales= \
	--fonts= \
	--install-modules="multiboot2 normal" \
	--modules= \
	--compress=xz
