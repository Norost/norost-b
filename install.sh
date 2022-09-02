#!/usr/bin/env bash

# Don't run this directly as root!
# Copy it to a safe location with proper permissions first.

[ -z "$1" ] || [ -z "$2" ] && echo Usage: $0 "<from> <to>" && exit 1

while true
do
	inotifywait -e create `dirname "$2"`
	if [ -e "$2" ]
	then
		sleep 0.5
		dd if="$1" of="$2"
	fi
done
