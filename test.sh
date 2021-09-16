#!/bin/sh

./mkkernel.sh

if grub-file --is-x86-multiboot2 target/x86_64-unknown-norostb/release/nora; then
	echo "Multiboot 2 OK" 
else
	echo "Multiboot 2 header is invalid"
fi
