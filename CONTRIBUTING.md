# Contributing to labeldeck

For a bug report, include your labeldeck version, operating system, reproduction
steps, and expected result. Discuss larger changes in an issue before writing
code. Keep each pull request focused on one fix or change, and open it against
`main`. Describe the problem, the resulting behaviour, and the checks you ran;
link the related issue. Report vulnerabilities through [SECURITY.md](SECURITY.md).

See the [README](README.md) for installation, CLI usage, and authentication.

## Local development

Use stable Rust with rustfmt and Clippy. The minimum supported Rust version is
**1.88.0**, declared in [Cargo.toml](Cargo.toml).

For the full local gate, install `cargo-make`, `cargo-nextest`, `cargo-audit`,
ShellCheck, PowerShell 7+ (`pwsh`), `actionlint`, and `zizmor`. Install the
Rust 1.88.0 toolchain for the MSRV check.

```sh
cargo build --locked
cargo make check
cargo make test
cargo make clippy
cargo make format
```

For a focused integration test, use the `main` test target:

```sh
cargo test --test main <test_name> --locked
```

Before submitting a PR, run:

```sh
cargo make verify
```

See [Makefile.toml](Makefile.toml) for the task definitions. The full gate checks
formatting, compilation, Clippy, tests, rustdoc, the release build, packaging,
MSRV, dependency advisories, installer and release-verifier tests, and workflow
linting. Packaging requires a committed working tree; during development, use
`cargo package --locked --allow-dirty` to check your pending changes, then rerun
`cargo make verify` after committing.

For an MSRV-sensitive change, check and test the locked dependency graph:

```sh
cargo +1.88.0 check --all-targets --all-features --locked
cargo +1.88.0 test --all-targets --all-features --locked
```

For coverage, install `cargo-llvm-cov` and the Rust `llvm-tools-preview` component.
Run `cargo make coverage` for `target/llvm-cov/coverage.lcov`, or
`cargo make coverage-html` for `target/llvm-cov/html`.

## Code and tests

Keep changes small, use typed errors, and preserve CLI compatibility and
deterministic output. Consider Linux, macOS, and Windows behaviour, including
paths and shell quoting. Avoid unnecessary dependencies and async infrastructure.
The crate forbids unsafe Rust.

Add regression tests for bug fixes and behaviour changes. Prefer integration
tests in `tests/suite/`; reuse the local mock GitHub API in `tests/common/` rather
than contacting GitHub. Keep fixtures deterministic, use temporary directories
with cleanup guards, and test pruning, overwrite protection, and partial failures
when your change affects them.

Update README and help text for user-visible changes. Leave `CHANGELOG.md` to the
maintainer. For TUI changes, include terminal screenshots or a short recording.

CI checks the Rust build and tests on Linux, macOS, and Windows, runs an MSRV
check on Linux, and audits workflows with Zizmor. Installer and verifier checks
vary by platform; see [the CI workflow](.github/workflows/ci.yml). Report local
checks you could not run and let CI cover the native platforms you lack.

## Commits

Follow the existing Conventional Commits and sign-off convention: use a scoped
subject such as `fix: preserve label associations` or `docs: clarify setup`, and
run `git commit -s` to add your `Signed-off-by` line. The maintainer uses GPG
signatures; contributors do not need to adopt that practice to submit a PR.
