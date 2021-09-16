#!/bin/sh

./mkkernel.sh

mkdir -p isodir/boot/grub
cp target/x86_64-unknown-norostb/release/nora isodir/boot/nora
cp boot/grub/grub.cfg isodir/boot/grub/grub.cfg
grub-mkrescue -o norost.iso isodir
