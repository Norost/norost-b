TARGET = x86_64-unknown-norostb

all: nora_scp

nora_scp: nora_ssh/target/$(TARGET)/release/nora_scp

nora_ssh/target/$(TARGET)/release/nora_scp: nora_ssh/ nora_ssh/nora_scp/src/main.rs
	cd nora_ssh/nora_scp && cargo b --target x86_64-unknown-norostb --release

nora_ssh/:
	git clone --depth 1 https://git.sr.ht/~demindiro/nora_ssh

clean:
	rm -rf nora_ssh
