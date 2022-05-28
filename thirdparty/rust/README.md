# Building the Rust toolchain

I'm still fumbling with automating the process of building the Rust toolchain
for custom targets, so here are manual instructions instead:

1. Clone the [Rust repository](github.com/rust-lang/rust/) and checkout commit `56fd680cf92`.
2. Copy `config.toml` to the cloned repository.
3. Apply `patch.diff`
4. Run `x.py b`
5. `rustup link dev-x86_64-unknown-norostb /path/to/rust/build/<your platform>/stage2`
   - `<your platform>` is `x86_64-unknown-linux-gnu` on Linux!

**Note:** `x.py dist` is broken due to a dependency on the `libc` crate, which isn't
configured yet for this target. To get `cargo-fmt` et al. to work, link it manually
to the toolchain folder (tools are located in `stage2-tools`).
Relevant issue: https://github.com/rust-lang/rust/issues/81431.
