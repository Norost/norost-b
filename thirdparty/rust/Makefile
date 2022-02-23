include ../../common.mk

#RUST_VERSION = 1.58.1
RUST_VERSION = nightly

RUSTC_HOST = x86_64-unknown-linux-gnu

JOBS ?= $(shell nproc)

build: rust

rust: # rust-$(RUST_VERSION)
	@echo Check the README for build instructions
	@exit 1
	cd $< && ./x.py setup# library/std
	exit 5
	cd $< && ./x.py setup \
		--target $(RUSTC_TARGET)
	exit 4
	cd $< && ./x.py build \
		--stage 0 \
		--target $(RUSTC_TARGET) \
		library/std
	exit 3
	cd $< && ./x.py \
		--target $(RUSTC_TARGET) \
		--release \
		--features "panic-unwind compiler-builtins-c compiler-builtins-mem"

rust-$(RUST_VERSION): rust-$(RUST_VERSION).tar.gz
	tar xzvf $<
	mv rustc-$(RUST_VERSION)-src $@

rust-$(RUST_VERSION).tar.gz:
	curl https://static.rust-lang.org/dist/rustc-$(RUST_VERSION)-src.tar.gz -o $@


clean: clean-build clean-unpacked

distclean: clean-build clean-archives clean-unpacked clean-all

clean-build:
	rm -rf $(BUILD)

clean-archives:
	rm -f rust-$(RUST_VERSION).tar.gz

clean-unpacked:
	rm -rf rust-$(RUST_VERSION)

clean-all-local: clean-build clean-unpacked
