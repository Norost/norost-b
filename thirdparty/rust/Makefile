URL = https://git.sr.ht/~demindiro/rust
RUSTC_HOST = x86_64-unknown-linux-gnu
OUTPUT = rust/build/$(RUSTC_HOST)/stage2/
TOOLS = $(OUTPUT)bin/

all: build link fix_tools

build: $(OUTPUT)

link: $(OUTPUT)
	rustup toolchain link dev-x86_64-unknown-norostb $(OUTPUT)

fix_tools: | $(TOOLS)rustfmt $(TOOLS)cargo-fmt $(TOOLS)cargo-clippy

clean:
	cd rust && ./x.py clean

clean-all:
	rm -rf rust/


$(TOOLS)rustfmt $(TOOLS)cargo-fmt $(TOOLS)cargo-clippy:
	ln -s ../../stage2-tools/$(RUSTC_HOST)/release/$(patsubst $(TOOLS)%,%,$@) $@

$(OUTPUT): rust/config.toml
	cd rust && ./x.py build

rust/config.toml: rust/
	cp config.toml $<config.toml

rust/:
	git clone --depth 1 $(URL) $@
	cd $@ && ./x.py setup library


.PHONY: all build clean clean-all fix_tools link
