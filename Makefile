MKFS.FAT = /sbin/mkfs.fat
SFDISK = /sbin/sfdisk
DD = dd

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
	$(MKFS.FAT) -F 12 $@

usb0:
	fallocate -l $$((256 * 512)) $@
	$(SFDISK) $@ < $@.sfdisk
	$(eval TMP := $(shell mktemp))
	fallocate -l $$((128 * 512)) $(TMP)
	$(MKFS.FAT) -F 12 $(TMP)
	$(DD) if=$(TMP) of=$@ bs=512 seek=40 conv=notrunc
	rm -f $(TMP)

clean:
	cargo clean
	cd tools && (if test -e nora_ssh; then cd nora_ssh && cargo clean; fi)

.PHONY: kernel boot run clean
