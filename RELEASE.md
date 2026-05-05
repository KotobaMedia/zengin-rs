# Release Runbook

This runbook covers publishing the workspace crates to crates.io with `cargo-release`:

- `zengin-fmt`: library crate
- `zengin-cli`: CLI crate, published as package `zengin-cli` and installed as binary `zengin`

`zengin-cli` depends on `zengin-fmt`. Use `cargo-release` for the normal release path so version bumps, the workspace dependency update, publish ordering, tagging, and pushing stay coordinated.

## Release Configuration

The release configuration lives in the root `Cargo.toml` under `[workspace.metadata.release]`.

Current policy:

- Shared workspace version.
- Release only from `main`.
- One consolidated release commit.
- One tag per repository release: `v{{version}}`.
- `cargo-release` publishes both workspace crates and updates `zengin-cli`'s `zengin-fmt` dependency version.

Check the effective configuration with:

```bash
cargo release config
```

## Metadata Checklist

Before publishing, confirm each package has crates.io metadata:

- `license`: inherited as `MIT`.
- `description`: shared for `zengin-fmt`, CLI-specific for `zengin-cli`.
- `repository`: inherited from the workspace.
- `readme`: `crates/zengin-fmt/README.md` symlinks to the repository README; `crates/zengin-cli/README.md` is CLI-specific.
- `documentation`: set to the relevant docs.rs page for each crate.
- `rust-version`: inherited as `1.88`, because the code uses Rust 2024 `let` chains.
- `keywords` and `categories`: set for crates.io discovery.

There is no `homepage` value because this repository does not have a distinct project site. Do not duplicate the repository URL as `homepage`.

Inspect metadata with:

```bash
cargo metadata --no-deps --format-version 1
```

## Preflight

Start from a clean worktree:

```bash
git status --short
```

Run the local checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo doc --workspace --no-deps
```

Make sure Cargo can authenticate to crates.io:

```bash
cargo login
```

In CI, set `CARGO_REGISTRY_TOKEN` instead.

Inspect package contents. Check for missing README files, accidental fixtures, or any real banking/customer data:

```bash
cargo package --list -p zengin-fmt
cargo package --list -p zengin-cli
```

## Dry Run

`cargo-release` is dry-run by default. Run the exact release command without `--execute` first.

Use the intended bump or exact version:

```bash
cargo release patch --workspace
cargo release minor --workspace
cargo release 0.2.0 --workspace
```

Review the dry-run output for:

- Workspace version bump.
- `zengin-cli` dependency update from the old `zengin-fmt` version to the new one.
- Packages to publish: `zengin-fmt`, then `zengin-cli`.
- Release commit.
- Tag name `vX.Y.Z`.
- Push target `origin`.

## Execute

Run the same command with `--execute`:

```bash
cargo release patch --workspace --execute
cargo release minor --workspace --execute
cargo release 0.2.0 --workspace --execute
```

`cargo-release` will commit the release changes, publish the crates, create the tag, and push to `origin`.

## Resume Or Repair

If the process fails after the release commit but before publishing completes, use the matching `cargo-release` step instead of raw `cargo publish`:

```bash
cargo release publish --workspace
cargo release publish --workspace --execute
```

If tagging or pushing failed after publish:

```bash
cargo release tag --workspace
cargo release tag --workspace --execute
cargo release push --workspace
cargo release push --workspace --execute
```

If owner enforcement is added to `[workspace.metadata.release]`, dry-run and execute it with:

```bash
cargo release owner --workspace
cargo release owner --workspace --execute
```

## Post-release Verification

Verify both packages are available:

```bash
cargo info zengin-fmt
cargo info zengin-cli
```

Install the CLI from crates.io and smoke-test the binary:

```bash
VERSION=0.2.0
cargo install zengin-cli --version "$VERSION" --locked
zengin --help
```

Create a GitHub release from the pushed `vX.Y.Z` tag and include the release notes.

## Recovery

Published crate files cannot be deleted or overwritten. If a release is broken:

1. Fix the issue.
2. Bump to a new patch version.
3. Release the fixed version with `cargo-release`.
4. Yank the broken version only if new users should stop selecting it:

   ```bash
   cargo yank --version X.Y.Z zengin-fmt
   cargo yank --version X.Y.Z zengin-cli
   ```

Yanking does not delete the crate and does not break existing lockfiles.

If a token or secret is accidentally published, revoke and rotate it immediately. Yanking is not a secret-removal mechanism.

## References

- `cargo-release`: https://docs.rs/crate/cargo-release/latest
- Cargo Book: Publishing on crates.io: https://doc.rust-lang.org/cargo/reference/publishing.html
- Cargo Book: manifest metadata: https://doc.rust-lang.org/cargo/reference/manifest.html
- Cargo Book: `rust-version`: https://doc.rust-lang.org/cargo/reference/rust-version.html
- Rust 1.88 release notes for Rust 2024 `let` chains: https://doc.rust-lang.org/stable/releases.html#version-1880-2025-06-26
