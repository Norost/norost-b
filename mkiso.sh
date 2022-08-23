#!/usr/bin/env bash

TARGET_BOOT=i686-unknown-none-norostbkernel
TARGET_KERNEL=x86_64-unknown-none-norostbkernel
TARGET_USER=x86_64-unknown-norostb
TOOLCHAIN=dev-x86_64-unknown-norostb

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

cp init.scf $A/init.scf
cp pci.scf $A/pci.scf
cp usb.scf $A/usb.scf
cp keyboard/azerty.scf $A/keyboard.scf
cp fs.example/cfg/userdb.scf $A/userdb.scf
cp fs.example/cfg/ssh.scf $A/sshd.scf
cp fs.example/cfg/ssh_secret.scf $A/sshd_secret.scf
mkdir $A/cfg
cp fs.example/users/test/cfg/password.scf $A/cfg/password.scf
cp fs.example/users/test/cfg/ssh.scf $A/cfg/ssh.scf

if [ "$1" == --release ] # stuff's broken otherwise
then
	export RUSTFLAGS="-Z unstable-options -C split-debuginfo=off"
else
	export RUSTFLAGS="-Z unstable-options -C split-debuginfo=unpacked"
fi


# Separate std and no_std builds because Cargo is retarded
cargo build $args \
	--target $TARGET_USER \
	--workspace \
	--exclude nora \
	--exclude noraboot \
	--exclude image_viewer \
	--exclude minish \
	--exclude ssh \
	--exclude driver_fs_fat \

cargo build $args \
	--target $TARGET_USER \
	--package image_viewer \
	--package minish \
	--package ssh \
	--package driver_fs_fat \

install () {
	cp target/$TARGET_USER/$build_dir/$2 $A/$1
}

install_ext () {
	(cd $2 && cargo +$TOOLCHAIN b $args --target $TARGET_USER)
	cp $2/target/$TARGET_USER/$build_dir/$1 $A/$1
}

install fs_fat             driver_fs_fat
install intel_hd_graphics  driver_intel_hd_graphics
install pci                driver_pci
install ps2                driver_ps2
install scancode_to_char   driver_scancode_to_char
install usb                driver_usb
install usb_kbd            driver_usb_kbd
install virtio_block       driver_virtio_block
install virtio_gpu         driver_virtio_gpu
install virtio_net         driver_virtio_net
install init               init
install gui_cli            gui_cli
install image_viewer       image_viewer
install minish             minish
install ssh                ssh
install static_http_server static_http_server
install window_manager     window_manager
(
	exit
	cd tools
	make nora_scp
	cp nora_ssh/target/x86_64-unknown-norostb/release/nora_scp $A/scp
)
install_ext userdb ../bin/userdb

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
