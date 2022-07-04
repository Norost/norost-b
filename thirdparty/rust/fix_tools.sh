#!/bin/sh

if test -z "$1"
then
	echo "Usage: $0 <path/to/toolchain>" 1>&1
	exit 1
fi

cd "$1/stage2/bin" || exit

for tool in rustfmt cargo-fmt cargo-clippy
do
	ln -s ../../stage2-tools/x86_64-unknown-linux-gnu/release/$tool
done
