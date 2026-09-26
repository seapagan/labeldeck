# labeldeck

Export, diff, and safely synchronize GitHub repository labels from a canonical JSON file.

`labeldeck` is a standalone cross-platform CLI (Linux, macOS, Windows) that keeps a GitHub repository's labels in line with a version-controlled canonical JSON file. It talks directly to the GitHub REST API — it does **not** require `gh`, `curl`, `jq`, Python, or Node at runtime — and uses the same TLS stack on every platform (rustls, no system OpenSSL).

## Why update-in-place matters

Some label tools "sync" by deleting every label and recreating them. On GitHub, **deleting a label removes it from every issue and pull request it is attached to**. Recreating a label with the same name does not restore those associations; you lose history and open issues silently lose their labels.

`labeldeck` never does this. It uses GitHub's label-update API to change an existing label's colour or description *in place*, so the label keeps its identity and every issue/PR association survives:

```text
missing on GitHub          → created
present but different      → updated in place (never renamed, never recreated)
identical                  → left untouched
present only on GitHub     → retained by default; deleted only when pruning
```

Because updates never rename, a difference that is only a change of name (for example `Bug` → `defect`) cannot be expressed by plain label JSON: it is applied as a create of the new name plus — only if pruning is enabled — a delete of the old one. There is no safe way to infer a rename from a name disappearing and another appearing, so `labeldeck` does not pretend to. Documented limitation, by design.

> **Pruning warning.** With pruning enabled, a label that exists on GitHub but not in your canonical file is **deleted**. Deleting a label removes it from existing issues and pull requests. Keep pruning off (the default) unless you accept that consequence — and preview with `--dry-run` or `labeldeck diff` first.

## Workflow

```text
export → edit/review the JSON (in a PR) → diff → sync
```

```console
# Capture the current labels of a repository as a canonical file
$ labeldeck export seapagan/labeldeck labels.json

# ...review/edit labels.json, commit it, get it code-reviewed...

# Preview what a sync would do (no changes, diff-style exit codes)
$ labeldeck diff labels.json seapagan/labeldeck

# Apply: create missing labels, update changed ones in place
$ labeldeck sync labels.json seapagan/labeldeck

# Also delete labels missing from the canonical file (destructive)
$ labeldeck sync labels.json seapagan/labeldeck --prune

# Preview pruning without doing anything
$ labeldeck sync labels.json seapagan/labeldeck --prune --dry-run
```

## CLI surface

```text
labeldeck export OWNER/REPO [FILE] [--force]
labeldeck diff FILE OWNER/REPO [--prune | --no-prune]
labeldeck sync FILE OWNER/REPO [--prune | --no-prune] [--dry-run]
labeldeck auth login [--token-stdin]
labeldeck auth logout
labeldeck auth status
```

### `export`

Writes the repository's labels as canonical JSON. With no `FILE` the JSON goes to standard output (nothing else is printed there, so it is safe to pipe). With a `FILE`, an existing file is **never** overwritten unless `--force` is given. Export works unauthenticated for public repositories (subject to GitHub's 60 requests/hour anonymous limit).

### `diff`

Compares the canonical file with the live repository and prints one line per label, without changing anything:

```text
CREATE bug (color d73a4a, description "Something isn't working")
UPDATE docs (color 0075ca -> 00ff00, description (none) -> "Documentation")
DELETE stale-label
RETAIN legacy-label (target-only; kept because pruning is disabled)
UNCHANGED feature
```

`DELETE` appears only where the effective prune setting would actually remove the label; otherwise the extra label is shown as `RETAIN`.

### `sync`

Applies the plan. Operations are ordered defensively: **all** creates and updates run first, and prune deletions start only after every one of them succeeded. Mutations pause briefly between requests per GitHub's rate-limit guidance, so very large syncs take a little longer and stay within GitHub's secondary limits.

GitHub's REST API is not transactional. If an operation fails mid-run, `labeldeck` stops, reports exactly what was applied, what failed, and what was skipped — it does not pretend to roll anything back.

`--dry-run` performs **zero** mutations: it reads the repository, prints the plan, and exits successfully.

### Exit codes

| Code | Meaning |
|------|---------|
| `0`  | Success. For `diff`: no differences under the effective prune setting. |
| `1`  | `diff` found differences (traditional diff-style semantics, suitable for CI). |
| `2`  | Error: usage, invalid file/config/repository spec, authentication, network, or API failure. |

## Canonical label file

A JSON array of objects with exactly three keys:

```json
[
  {
    "name": "bug",
    "color": "d73a4a",
    "description": "Something isn't working"
  },
  {
    "name": "docs",
    "color": "0075ca",
    "description": null
  }
]
```

- **name** — required, non-empty, at most 50 characters (GitHub's observed limit; it is not officially documented). Duplicate names — including duplicates that differ only in case, which GitHub treats as the same label — are rejected before any mutation.
- **color** — required, six hexadecimal digits. A leading `#` and any letter case are accepted on input; the canonical form stored by GitHub and written by `export` is lowercase without `#` (for example `d73a4a`). Three-digit CSS shorthand is rejected, as GitHub rejects it too.
- **description** — required key, string or `null` (`""` and `null` both mean "no description"). At most 100 characters.

Unknown keys are rejected, so a typo like `"colour"` fails loudly instead of silently dropping data. `export` output is deterministic: labels sorted by name, fixed field order, two-space indentation, one trailing newline — ready for code review.

An empty canonical file (`[]`) is valid. With pruning enabled it means "delete every label" — deliberate, dangerous, and exactly why pruning is off by default.

## Authentication

`labeldeck` resolves credentials in this order:

```text
LABELDECK_TOKEN → GH_TOKEN → GITHUB_TOKEN → stored labeldeck token → anonymous
```

- Environment variables suit scripts and CI; `LABELDECK_TOKEN` wins so you can override a ambient `GH_TOKEN` when needed. Blank values count as unset.
- Anonymous access works for public repositories (reads only).
- Private repositories and all write operations require a token.

Tokens are never accepted as command-line arguments, where process listings could observe them. `labeldeck auth login` prompts for the token with the input hidden (`--token-stdin` reads it from standard input in scripts), validates it against GitHub, and offers to store it.

Token permissions needed:

| Use | Classic PAT | Fine-grained PAT |
|-----|-------------|------------------|
| Read labels (public repo) | none | none |
| Read labels (private repo) | `repo` (or `public_repo` for public only) | Issues: read + Metadata: read |
| Create/update/delete labels | `repo` (or `public_repo` for public only) | Issues: write + Metadata: read |

### Token storage and security

- The stored token lives in its own file (`token`) inside the labeldeck configuration directory — **never** in `config.toml`.
- On Linux/macOS the file is created with mode `0600` (owner read/write only) inside a `0700` directory.
- On Windows the file is written with the default protections of your user profile directory; no stronger ACL guarantee is claimed or implemented.
- `auth status` reports whether a token is available and *where it came from* (which variable, or the stored-file path). It never prints the token itself, and tokens never appear in diagnostics, errors, or debug output.
- `auth logout` deletes the stored token. Environment-provided tokens are unaffected.

## Configuration

`config.toml` in the labeldeck configuration directory. It is deliberately tiny — one key today:

```toml
prune = true
```

- `prune` (optional, default `false`): delete target-only labels during `sync`.
- Precedence: `--prune`/`--no-prune` on the command line beats the config file, which beats the built-in default. A user who has `prune = true` can still pass `--no-prune` for one invocation.
- Unknown keys and malformed files are hard errors with the file path and a precise reason.

Configuration directory locations:

| Platform | Path |
|----------|------|
| Linux | `~/.config/labeldeck` (`$XDG_CONFIG_HOME` honoured) |
| macOS | `~/Library/Application Support/labeldeck` |
| Windows | `%APPDATA%\labeldeck` |

`LABELDECK_CONFIG_DIR` overrides the location (used by tests and sandboxed automation). Files: `config.toml` for preferences, `token` for the stored credential.

Two other environment variables are understood:

- `LABELDECK_API` — override the GitHub API base URL (an internal/testing escape hatch; unset for normal use).
- `LABELDECK_CONFIG_DIR` — see above.

## Installation

### Prebuilt binaries (recommended)

Download from the [releases page](https://github.com/seapagan/labeldeck/releases). Every archive has a `.sha256` sidecar; verify before use:

```console
$ sha256sum -c labeldeck-v0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Published targets:

| Target | Archive |
|--------|---------|
| `x86_64-unknown-linux-gnu` | `.tar.gz` |
| `x86_64-unknown-linux-musl` (static) | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | `.tar.gz` |
| `aarch64-unknown-linux-musl` (static) | `.tar.gz` |
| `x86_64-apple-darwin` | `.tar.gz` |
| `aarch64-apple-darwin` | `.tar.gz` |
| `x86_64-pc-windows-msvc` | `.zip` |
| `aarch64-pc-windows-msvc` | `.zip` |

Linux GNU builds are produced in a manylinux 2.28 container and are verified not to require a glibc newer than 2.28; on older systems the installer automatically selects the fully static musl build instead.

#### Unix installer (Linux/macOS)

```console
$ curl -fsSL https://raw.githubusercontent.com/seapagan/labeldeck/main/install.sh | sh
```

The installer selects the right architecture and libc, verifies the SHA-256 before extracting, runs the binary's `--version` to confirm it works, and only then replaces any existing installation (atomically, on the same filesystem). An explicit version can be pinned with `LABELDECK_VERSION=v0.1.0`, and `LABELDECK_LIBC=gnu|musl` overrides libc selection on Linux.

#### Windows installer (PowerShell)

```powershell
PS> Invoke-WebRequest https://raw.githubusercontent.com/seapagan/labeldeck/main/install.ps1 -OutFile install.ps1
PS> ./install.ps1
```

or, if your execution policy allows:

```powershell
PS> irm https://raw.githubusercontent.com/seapagan/labeldeck/main/install.ps1 | iex
```

The same guarantees: architecture detection (x64 and ARM64), SHA-256 verification with `Get-FileHash`, the candidate's `--version` is executed before anything replaces an existing binary, and failures never destroy a working installation. `LABELDECK_VERSION` pins a version here too.

### cargo-binstall

```console
$ cargo binstall labeldeck
```

### From source

```console
$ cargo install --locked --git https://github.com/seapagan/labeldeck
```

Requires a Rust toolchain; see the MSRV below.

## Minimum Supported Rust Version

The proven MSRV is **1.85.1** (determined with `cargo-msrv` and verified against the full gate on that exact toolchain; 1.85.x is the Rust 2024 edition baseline). It is verified in CI on every push, and the dependency graph uses the MSRV-aware resolver so dependency updates cannot silently raise it past the declared value.

## Direct GitHub API usage

`labeldeck` implements the documented REST endpoints itself (list with `Link`-header pagination, create, in-place update, delete) over a small synchronous HTTP client with rustls. There is no runtime dependency on the GitHub CLI or any other external tool, and no async runtime. Requests follow GitHub's conventions: `User-Agent`, `Accept: application/vnd.github+json`, the pinned `X-GitHub-Api-Version` header, and Bearer authentication.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option, the standard Rust ecosystem licence pair. Both licence texts ship in the repository and in release archives.
