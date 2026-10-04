# Contributing to labeldeck

Thanks for considering a contribution.

## Development setup

You need a recent stable Rust toolchain (see `rust-version` in
`Cargo.toml` for the MSRV) plus these tools for the full local gate:

```console
$ cargo install cargo-make cargo-nextest cargo-llvm-cov
```

`actionlint`, `zizmor`, `shellcheck`, and `pwsh` (PowerShell 7+) are also
required for the complete verification gate; on Ubuntu they are available
via the usual package managers.

## Everyday commands

```console
$ cargo make check      # cargo check, all targets/features
$ cargo make test       # cargo-nextest
$ cargo make clippy     # warnings denied
$ cargo make format     # rustfmt check
$ cargo make coverage   # LCOV + HTML coverage
```

Before opening a PR, run the full gate:

```console
$ cargo make verify
```

`verify` runs formatting, check, clippy, tests, docs, release build,
package verification, MSRV verification, installer tests (POSIX and
PowerShell), release-verifier tests, actionlint, and zizmor. CI runs the
same gates on Linux, macOS, and Windows.

## Project layout

- `src/labels.rs`, `src/canonical.rs` — label model and canonical JSON
- `src/plan.rs`, `src/sync.rs` — planning and safe execution
- `src/edit/` — document/history, rename planning, paced execution, colour adapter, and testable TUI
- `src/github.rs` — direct GitHub REST client (ureq + rustls)
- `src/config.rs`, `src/auth.rs` — configuration and token handling
- `src/cli.rs`, `src/commands/` — clap interface and command UX
- `tests/` — integration tests, including the local mock GitHub API

## Testing rules

- Tests must not contact GitHub. Use the in-process mock server in
  `tests/common/`.
- Behaviour that guards destructive operations (pruning, overwrite
  protection, partial-failure reporting) deserves direct tests.
- The full suite must pass on Linux, macOS, and Windows.

## Commits

Conventional Commits style (`feat:`, `fix:`, `chore:`, …), signed off
with `git commit -s`.

## Releases

Releases are cut by pushing a `vX.Y.Z` tag matching the crate version.
The release workflow builds all targets, verifies binaries, produces
checksummed archives with GitHub artifact attestations, and publishes an
immutable release. See `.github/workflows/release.yml` and the
installation section of the README. Do not publish releases manually.
