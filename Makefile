# There's a bug in cargo that causes panics when using `forced-target`
# Use Makefiles as workaround for now
build: kernel boot

kernel:
	cargo b --bin nora

boot:
	cargo b --bin noraboot

run:
	./run.sh

disk0:
	fallocate -l $$((128 * 512)) $@
	/sbin/mkfs.fat -F 12 $@

.PHONY: kernel boot run
