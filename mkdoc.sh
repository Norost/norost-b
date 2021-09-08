#!/usr/bin/sh

set -x

IN="kernel driver userspace"

IN_DIR="Design"
OUT_DIR="build/doc/design"

mkdir -p "$OUT_DIR"

for i in $IN
do
	rst2html5.py "Design/$i/Index.rst" "$OUT_DIR/$i.html"
done
