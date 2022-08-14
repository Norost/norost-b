#!/usr/bin/env bash

TARGET_BOOT=i686-unknown-none-norostbkernel
TARGET_KERNEL=x86_64-unknown-none-norostbkernel
TARGET_USER=x86_64-unknown-norostb

set -e
set -x

if [ "$1" == --release ]
then
	args="--release"
	build_dir=release
else
	args=
	build_dir=debug
fi

. ./env.sh

./mkkernel.sh $args || exit $?
./mkboot.sh $args || exit $?

O=$(mktemp -d)
A=$(mktemp -d)
trap 'rm -rf "$O" "$A"' EXIT

mkdir -p $O/boot/grub
cp target/$TARGET_KERNEL/$build_dir/nora $O/boot/nora
cp target/$TARGET_BOOT/$build_dir/noraboot $O/boot/noraboot
cp boot/$ARCH/grub/grub.cfg $O/boot/grub/grub.cfg

cp init.toml $A/init.toml
cp usb.scf   $A/usb.scf
cp keyboard/azerty.scf $A/keyboard.scf
cp -r ssh    $A/ssh_conf

if [ "$1" == --release ] # stuff's broken otherwise
then
	export RUSTFLAGS="-Z unstable-options -C split-debuginfo=off"
else
	export RUSTFLAGS="-Z unstable-options -C split-debuginfo=unpacked"
fi

install () {
	(cd $1/$2 && cargo build $args --target $TARGET_USER)
	cp target/$TARGET_USER/$build_dir/$3 $A/$2
}

#install drivers fs_fat             driver_fs_fat
#install drivers intel_hd_graphics  driver_intel_hd_graphics
install drivers ps2                driver_ps2
install drivers scancode_to_char   driver_scancode_to_char
install drivers usb                driver_usb
install drivers usb_kbd            driver_usb_kbd
#install drivers virtio_block       driver_virtio_block
#install drivers virtio_gpu         driver_virtio_gpu
#install drivers virtio_net         driver_virtio_net
install base    init               init
#install base    gui_cli            gui_cli
#install base    image_viewer       image_viewer
#install base    jail               jail
install base    minish             minish
#install base    ssh                ssh
#install base    static_http_server static_http_server
#install base    window_manager     window_manager
(
	exit
	cd tools
	make nora_scp
	cp nora_ssh/target/x86_64-unknown-norostb/release/nora_scp $A/scp
)

./tools/nrofs.py -rv -C $A $O/boot/norost.nrofs .

# Note: make sure grub-pc-bin is installed! Otherwise QEMU may hang on
# "Booting from disk" or return error code 0009
grub-mkrescue -o norost.iso $O \
	--locales= \
	--fonts= \
	--install-modules="multiboot2 normal" \
	--modules= \
	--compress=xz

./tools/nrofs.py -lv $O/boot/norost.nrofs
