# ZenKit Rust

Rust readers, writers and a Daedalus runtime for Gothic I and II archives,
assets, object graphs, worlds and savegames. Tested on Windows x64
with Rust 1.99.0.

Start with the [crate guide](crates/zenkit/README.md) and
[examples](crates/zenkit/examples). Ordinary and Union compressed VDF support
is included by default.

The crate carries its own [license](crates/zenkit/LICENSE).

## Building

Requires Rust 1.99.0, selected automatically by `rust-toolchain.toml`.

Run from the repository root:

```powershell
# Debug build (target/debug/)
cargo build --workspace --locked

# Optimized build (target/release/)
cargo build --workspace --release --locked
```

To build the example programs in `target/release/examples/`:

```powershell
cargo build -p zenkit --examples --release --locked
```

On Windows with MSVC, linking the examples and tests needs the MSVC linker
and Windows SDK.

## Verification

Use Rust 1.99.0:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test --workspace --doc --locked
cargo doc --workspace --no-deps --locked
```

Synthetic test fixtures live under `crates/zenkit/tests/fixtures/`.
Original game data and derived payloads are not included.

GitHub Actions runs these checks on Windows x64, including documentation generation
with warnings denied.
