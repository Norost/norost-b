#!/bin/sh

./mkiso.sh

qemu-system-x86_64 -drive format=raw,file=norost.iso
