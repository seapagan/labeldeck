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

Because sync/copy updates never rename, a difference that is only a change of name (for example `Bug` → `defect`) cannot be expressed by plain label JSON: it is applied as a create of the new name plus — only if pruning is enabled — a delete of the old one. There is no safe way to infer a rename from a name disappearing and another appearing, so `labeldeck` does not pretend to. Documented limitation, by design.

> **Pruning warning.** With pruning enabled, a label that exists on GitHub but not in your canonical file is **deleted**. Deleting a label removes it from existing issues and pull requests. Keep pruning off (the default) unless you accept that consequence — and preview with `--dry-run` or `labeldeck diff` first.

## Workflow

```text
export → labeldeck edit → review the JSON (in a PR) → diff → sync
```

```console
# Capture the current labels of a repository as the canonical file
$ labeldeck export seapagan/keyhold

# Edit the local deck, then commit it and get it code-reviewed
$ labeldeck edit

# Preview what a sync would do (no changes, diff-style exit codes)
$ labeldeck diff seapagan/lsplus

# Apply: create missing labels, update changed ones in place
$ labeldeck sync seapagan/lsplus

# Also delete labels missing from the canonical file (destructive)
$ labeldeck sync seapagan/lsplus --prune

# Preview pruning without doing anything
$ labeldeck sync seapagan/lsplus --prune --dry-run

# Copy a template repository's labels straight onto a new repository
$ labeldeck copy seapagan/template seapagan/new-project

# Preview that copy (or its pruning) without changing anything
$ labeldeck copy seapagan/template seapagan/new-project --prune --dry-run

# Establish your personal default deck once
$ labeldeck export seapagan/labeldeck --global

# Use a different canonical file for any of the above
$ labeldeck diff seapagan/lsplus --file other.json
```

## CLI surface

```text
labeldeck [--no-proxy] edit [OWNER/REPO | --global | --file PATH]
labeldeck [--no-proxy] export OWNER/REPO [--file PATH | --global] [--force]
labeldeck [--no-proxy] diff OWNER/REPO [--file PATH] [--prune | --no-prune]
labeldeck [--no-proxy] sync OWNER/REPO [--file PATH] [--prune | --no-prune] [--dry-run]
labeldeck [--no-proxy] copy SOURCE TARGET [--prune | --no-prune] [--dry-run]
labeldeck [--no-proxy] auth login [--token-stdin]
labeldeck auth logout
labeldeck auth status
```

All repository commands take `OWNER/REPO` arguments (`copy` takes two:
`SOURCE` then `TARGET`; URLs are not accepted).
`diff` and `sync` resolve the canonical deck as `--file PATH` → `./labels.json` →
`<config dir>/labels.json` (the global default deck). The `--no-proxy`
flag (usable anywhere) bypasses any configured HTTP proxy for that
invocation; by default the normal proxy environment
(`HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY`) is honoured.

### `export`

Writes the repository's labels as canonical JSON to `./labels.json`. `--global` instead writes your **personal default deck** to `<labeldeck config directory>/labels.json` (the directory is created if needed) — see [the global default deck](#the-global-default-deck). An existing destination file is **never** overwritten unless `--force` is given, and that protection applies equally to the local default, `--global`, and any explicit `--file PATH`. `--file -` writes the JSON to standard output instead — nothing else is printed there, so it is safe to pipe — and cannot be combined with `--force` or `--global`, which are rejected as usage errors. Export works unauthenticated for public repositories (subject to GitHub's 60 requests/hour anonymous limit).

### `diff`

Compares the resolved deck (`--file PATH` → `./labels.json` → global default) with the live repository and prints one line per label, without changing anything:

```text
CREATE bug (color d73a4a, description "Something isn't working")
UPDATE docs (color 0075ca -> 00ff00, description (none) -> "Documentation")
DELETE stale-label
RETAIN legacy-label (target-only; kept because pruning is disabled)
UNCHANGED feature
```

`DELETE` appears only where the effective prune setting would actually remove the label; otherwise the extra label is shown as `RETAIN`.

### `edit`

`labeldeck edit` opens **only `./labels.json`**, with no global fallback.
`labeldeck edit --global` opens only the actual global deck, even if a local
one exists. `labeldeck edit --file PATH` edits exactly that existing canonical
file. Missing or invalid decks fail with guidance; edit never creates a missing
deck. `--file -` is invalid. These selectors and `OWNER/REPO` are mutually exclusive.

`labeldeck edit OWNER/REPO` fetches live labels using write credentials before
opening the editor. It uses the same token precedence and first-use login as
sync; `--no-proxy` applies. Input and output must be interactive terminals.

Edits remain in memory. **Cancel** leaves the source untouched. **Apply** is
disabled until the final label set differs from the baseline. Ctrl-S or Apply
validates the final set and opens a centred confirmation modal over the editor.
No source mutation occurs before confirmation. Confirming restores the terminal before
writing the deck or issuing paced GitHub mutations, with progress on stderr.
Undo/Redo operate on committed edits, additions, and deletions, not keystrokes
or filter changes. Saving a label's three-field form is one undoable edit.
A new committed edit after Undo clears Redo.

| Keys | Action |
| --- | --- |
| Up/Down, PageUp/PageDown | Select a label |
| Enter or `e` | Open the selected label's Name, Color, and Description form |
| `n`, Delete | Open a new-label form or mark the selected label for deletion |
| `/` | Filter by case-insensitive name/description substring |
| Up/Down or Tab/Shift-Tab in the form | Focus another field without saving |
| Left/Right, Home/End, Backspace/Delete in the form | Move the cursor or edit text |
| Enter in the form | Validate and save all three fields together |
| Esc in the form | Cancel the entire edit, including a pending new label |
| Enter / Esc in the filter | Accept the filter / restore the previous filter |
| Ctrl-Z / Ctrl-Y | Undo / Redo; discard uncommitted form input first |
| Tab/Shift-Tab at list level | Cycle the compact footer buttons |
| Ctrl-S / Apply | Open confirmation when semantic changes exist |
| `q` or Esc at list level, Ctrl-C anywhere | Cancel |

Buttons, list rows, and form fields support mouse clicks; the mouse wheel
navigates labels. The footer groups Undo, Redo, Apply, and Cancel on the left
and shows their shortcuts directly. The form marks its focused field with a
local highlight and a selection marker, beside a continuous left border.
Colour previews use a small `■` swatch and update as soon as a draft is valid;
incomplete colours are allowed while typing and validated when saving the form.
Confirmation defaults to Back. Left/Right or Tab/Shift-Tab selects Apply/Back,
Enter activates the choice, and Esc returns to the editor.
Colour swatches respect terminal capabilities through colored_text and
`NO_COLOR`; hex values remain visible and stored colours are unchanged.

Live renames update labels in place, preserving issue/PR associations. Rename
chains and cycles use collision-safe ordering and temporary names where needed.
**Deleting a live label removes it from existing issues and pull requests.**
Deletes run last, after all non-destructive operations succeed. Final names
must be unique ignoring case; temporary duplicates while staging a swap are
allowed, but cannot be applied.

Apply refuses a file whose bytes changed while the editor was open. Live Apply
refetches labels and refuses any semantic baseline change, ignoring API list
order. It never silently merges concurrent edits. These guards are optimistic;
a race after the final check is still possible. GitHub operations are not
transactional: on failure, exit code 2 and applied/failed/skipped diagnostics
identify the actual names, including surviving temporary names. Applied
operations remain in effect; no rollback is attempted.

No-op Apply leaves files byte-for-byte unchanged and issues no mutation
requests. File editing does not teach sync/copy to infer renames; use live edit
when preserving an existing label's identity across a rename matters.

### `sync`

Applies the plan read from the resolved deck (`--file PATH` → `./labels.json` → global default). Operations are ordered defensively: **all** creates and updates run first, and prune deletions start only after every one of them succeeded. Each mutation after the first waits about one second before its request, per GitHub's rate-limit guidance, so very large syncs take a little longer and stay within GitHub's secondary limits.

GitHub's REST API is not transactional. If an operation fails mid-run, `labeldeck` stops, reports exactly what was applied, what failed, and what was skipped — it does not pretend to roll anything back.

`--dry-run` performs **zero** mutations: it reads the repository, prints the plan, and exits successfully.

### `copy`

Copies the labels currently configured on one repository directly to another — no local canonical file is involved or resolved. `labeldeck copy SOURCE TARGET` reads `SOURCE`'s labels, reads `TARGET`'s labels, and then applies exactly the same reconciliation `sync` uses: missing labels are created, changed labels are updated in place, and target-only labels are kept unless pruning is enabled (with the same `--prune`/`--no-prune`/`config.toml` precedence). Both repositories are read completely before any mutation starts, and `SOURCE` is **only ever read** — it is never modified, and a failure fetching either repository leaves `TARGET` untouched.

`--dry-run` performs **zero** mutations: it prints the plan and the command to apply it. A normal copy needs write access to `TARGET` and read access to `SOURCE` with the same token; a public `SOURCE` (and public `TARGET` for `--dry-run`) can be read anonymously. Copying a repository onto itself (`labeldeck copy OWNER/REPO OWNER/REPO`, including case differences) is rejected as a usage error.

### Exit codes

| Code | Meaning |
|------|---------|
| `0`  | Success. For `diff`: no differences under the effective prune setting. |
| `1`  | `diff` found differences (traditional diff-style semantics, suitable for CI). |
| `2`  | Error: usage, invalid file/config/repository spec, authentication, network, or API failure. |

## The global default deck

Keep one reusable personal label set in your labeldeck configuration
directory (`~/.config/labeldeck/labels.json` on Linux,
`~/Library/Application Support/labeldeck/labels.json` on macOS,
`%APPDATA%\labeldeck\labels.json` on Windows; `LABELDECK_CONFIG_DIR`
overrides the location):

```console
labeldeck export seapagan/labeldeck --global
```

`diff` and `sync` then use a repository-local `./labels.json` when one
exists and automatically fall back to this global deck when it does not
— so `labeldeck diff seapagan/foo` works in any directory you have not
given its own deck.

Precedence and safety rules:

```text
--file PATH → ./labels.json → <config dir>/labels.json
```

- An explicit `--file` is authoritative: if that file is missing,
  unreadable, or invalid, the command fails with that path — it never
  silently falls back to any default.
- A present-but-invalid local `./labels.json` is likewise an error; the
  global deck is used only when the local default is genuinely absent.
- When neither default exists, the error names both checked locations
  and suggests `labeldeck export OWNER/REPO` (local),
  `labeldeck export OWNER/REPO --global`, or `--file PATH`.
- Overwrite protection (`--force`) applies equally to the local deck,
  the global deck, and explicit `--file` paths; a plain
  `labeldeck export OWNER/REPO` never modifies the global deck.

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

Tokens are never accepted as command-line arguments, where process listings could observe them. `labeldeck auth login` prompts for the token with the input hidden (`--token-stdin` reads it from standard input in scripts), validates it against GitHub, and stores it — persistence is the purpose of the command, so there is no extra confirmation. The interactive first-use flow that `sync` offers when no token exists behaves differently: it prompts securely, validates, then asks whether to store the token for future use (defaulting to yes). Answering `n` keeps the token in memory for that run only and writes nothing to disk. Environment-provided tokens are never persisted by any command.

Token permissions needed:

| Use | Classic PAT | Fine-grained PAT |
|-----|-------------|------------------|
| Read labels (public repo) | none | none |
| Read labels (private repo) | `repo` (or `public_repo` for public only) | Issues: read + Metadata: read |
| Create/update/delete labels | `repo` (or `public_repo` for public only) | Issues: write + Metadata: read |

For `copy`, one token covers both sides: it needs write access to `TARGET` and (for a private `SOURCE`) read access to it.

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

- `prune` (optional, default `false`): delete target-only labels during `sync` and `copy`.
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

Proxy handling: standard proxy environment variables
(`HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY`, upper or lower case)
are honoured by default. Pass `--no-proxy` to bypass every proxy for a
single invocation; there is no persistent proxy configuration.

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

The MSRV is **1.88.0**, declared once in `Cargo.toml` (`rust-version`) as the single source of truth. The local gate (`cargo make msrv`) and the CI MSRV job both read that value directly from `Cargo.toml`, so no duplicate copy can drift. It is verified in CI on every push, and the dependency graph uses the MSRV-aware resolver so dependency updates cannot silently raise it past the declared value.

## Direct GitHub API usage

`labeldeck` implements the documented REST endpoints itself (list with `Link`-header pagination, create, in-place update, delete) over a small synchronous HTTP client with rustls. There is no runtime dependency on the GitHub CLI or any other external tool, and no async runtime. Requests follow GitHub's conventions: `User-Agent`, `Accept: application/vnd.github+json`, the pinned `X-GitHub-Api-Version` header, and Bearer authentication.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option, the standard Rust ecosystem licence pair. Both licence texts ship in the repository and in release archives.
