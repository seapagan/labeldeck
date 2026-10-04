# labeldeck

Export, edit, compare, copy, and synchronize GitHub repository labels.
`labeldeck` runs on Linux, macOS, and Windows as a standalone CLI.

## Updating labels safely

`labeldeck` updates existing labels in place, preserving their issue and pull
request associations. It creates missing labels and keeps extra labels unless
you enable pruning.

> **Pruning deletes labels and removes them from existing issues and pull requests.**
> Preview with `--dry-run` or `labeldeck diff` before using `--prune`.

To rename a label while preserving its associations, use `labeldeck edit OWNER/REPO`.
`sync` and `copy` treat a name change as a new label and keep the old one unless
you enable pruning.

## Workflow

```console
# Export a repository's labels to labels.json
$ labeldeck export seapagan/keyhold

# Edit the file
$ labeldeck edit

# Preview changes to a target repository
$ labeldeck diff seapagan/lsplus

# Apply the changes
$ labeldeck sync seapagan/lsplus
```

## Commands

| Command | Use |
|---------|-----|
| `export OWNER/REPO` | Save repository labels to a JSON file |
| `diff OWNER/REPO` | Compare a deck with repository labels |
| `edit` | Edit a local or global deck, or live repository labels |
| `sync OWNER/REPO` | Apply a deck to a repository |
| `copy SOURCE TARGET` | Copy labels between repositories |
| `auth login`, `auth status`, `auth logout` | Manage credentials |

Use `labeldeck --help` or `labeldeck COMMAND --help` for all options.
Specify repositories as `OWNER/REPO`, not URLs.

### `export`

`export` writes deterministic, name-sorted JSON suitable for version control and code review.

Export writes to `./labels.json` by default. Choose another destination with
`--file PATH`, or save a [global default deck](#the-global-default-deck) with `--global`:

```console
$ labeldeck export seapagan/keyhold --file team-labels.json
$ labeldeck export seapagan/keyhold --global
```

Use `--force` to overwrite an existing destination. To pipe JSON to another
command, use `--file -`; this option cannot combine with `--force` or `--global`.
You can export public repositories without a token, subject to GitHub's anonymous
rate limit.

### `diff`

Compare a [selected deck](#the-global-default-deck) with the live repository
without changing either:

```console
$ labeldeck diff seapagan/lsplus
$ labeldeck diff seapagan/lsplus --file team-labels.json --prune
```

The output shows one action per label:

```text
CREATE bug (color d73a4a, description "Something isn't working")
UPDATE docs (color 0075ca -> 00ff00, description (none) -> "Documentation")
DELETE stale-label
RETAIN legacy-label (target-only; kept because pruning is disabled)
UNCHANGED feature
```

With pruning enabled, extra labels appear as `DELETE`; otherwise they appear as
`RETAIN`. See [exit codes](#exit-codes) for use in scripts and CI.

### `edit`

Open an existing deck or edit live repository labels:

```console
$ labeldeck edit                       # Local labels.json
$ labeldeck edit --global              # Global default deck
$ labeldeck edit --file team-labels.json
$ labeldeck edit seapagan/keyhold       # Live GitHub labels
```

Choose one source. Local edit opens `./labels.json` without a global fallback;
`--global` and `--file PATH` open the specified deck. Edit requires an existing,
valid deck and does not accept `--file -`. Use an interactive terminal of at least
48 columns by 16 rows. Live editing requires [write credentials](#authentication).

Select a label and press Enter to edit its Name, Color, and Description. Press
Enter in the form to save your pending changes, or Esc to discard that form.
Use Undo and Redo for saved edits, additions, and deletions.

Press Ctrl-S or click Apply to review the confirmation, then choose Apply to
write your changes. Apply is available after you make changes. Cancel exits
without applying them.

| Keys | Action |
|------|--------|
| Up/Down, PageUp/PageDown | Select a label |
| Enter or `e` | Edit the selected label |
| `n` | Add a label |
| Delete at list level | Delete the selected label |
| `/` | Filter labels by case-insensitive name substring |
| Up/Down or Tab/Shift-Tab in the form | Select another field |
| Left/Right, Home/End, Backspace/Delete in the form | Move the cursor or edit text |
| Enter in the form | Save all three fields |
| Esc in the form | Cancel the form, including a new label |
| Enter / Esc in the filter | Accept the filter / restore the previous filter |
| Ctrl-Z / Ctrl-Y | Undo / Redo; discard unsaved form input first |
| Tab/Shift-Tab at list level | Cycle buttons |
| Ctrl-S / Apply | Open the Apply confirmation |
| `q` or Esc at list level, Ctrl-C anywhere | Cancel |

You can click rows, buttons, and fields, or scroll labels with the mouse wheel.
In the confirmation, use Left/Right or Tab/Shift-Tab to select Apply or Back,
then press Enter. Esc returns to the editor.

Enter six hex digits for Color, up to 50 characters for Name, and up to 100 for
Description. The editor shows validation errors below the fields. Label names
must be unique ignoring case before you apply changes.

**Deleting a live label removes it from existing issues and pull requests.**
Apply checks for source changes and refuses to proceed if it detects them.
If a GitHub operation fails during Apply, review the reported results before
retrying: completed changes remain in effect.

### `sync`

Apply a [selected deck](#the-global-default-deck) to a repository:

```console
$ labeldeck sync seapagan/lsplus
$ labeldeck sync seapagan/lsplus --file team-labels.json
$ labeldeck sync seapagan/lsplus --prune --dry-run
```

Use `--dry-run` to preview changes. Use `--prune` to delete labels absent from
the deck, or `--no-prune` to override a pruning preference in your configuration.

If an operation fails, labeldeck stops and reports completed, failed, and skipped
changes. Completed changes remain in effect.

### `copy`

Copy labels from one repository to another without a local JSON file:

```console
$ labeldeck copy seapagan/template seapagan/new-project
$ labeldeck copy seapagan/template seapagan/new-project --prune --dry-run
```

Copy updates the target and leaves the source unchanged. Extra target labels
remain unless you enable pruning. Use `--dry-run` to preview the changes.
The same [authentication](#authentication) token must cover both repositories.

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success. For `diff`: no differences under the effective prune setting. |
| `1` | `diff` found differences. |
| `2` | Error: usage, invalid file/config/repository spec, authentication, network, or API failure. |

## The global default deck

Save a reusable label set in your [configuration directory](#configuration):

```console
$ labeldeck export seapagan/labeldeck --global
```

`diff` and `sync` select their deck in this order:

```text
--file PATH → ./labels.json → <config dir>/labels.json
```

They use the global deck if no local `labels.json` exists. A missing or invalid
explicit file, or an unreadable or invalid local deck, causes an error rather
than a fallback. `edit` requires `--global` to open the global deck.

## Canonical label file

Use a JSON array of label objects:

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

All three keys are required; unknown keys cause an error.

- **name**: non-empty, at most 50 characters. Names must be unique ignoring case.
- **color**: six hex digits. File input accepts a leading `#` and uppercase letters;
  export writes lowercase digits without `#`.
- **description**: a string of at most 100 characters, or `null`. Use `""` or `null`
  for no description.

An empty deck (`[]`) is valid. **Syncing it with pruning enabled deletes all labels.**

## Authentication

```console
$ labeldeck auth login
$ labeldeck auth status
$ labeldeck auth logout
```

Login prompts for a hidden token, validates it, and stores it. Use
`labeldeck auth login --token-stdin` to supply a token through standard input.
For scripts and CI, set a token environment variable. Credential precedence is:

```text
LABELDECK_TOKEN → GH_TOKEN → GITHUB_TOKEN → stored labeldeck token → anonymous
```

Blank values count as unset. Environment-provided tokens stay out of the stored
token file. If sync prompts you for a token on first use, you can decline to save
it and use it for that run.

Public repositories allow anonymous reads. Private repositories and write
operations require a token with these permissions:

| Use | Classic PAT | Fine-grained PAT |
|-----|-------------|------------------|
| Read labels (public repo) | none | none |
| Read labels (private repo) | `repo` | Issues: read + Metadata: read |
| Create/update/delete labels | `repo` (or `public_repo` for public only) | Issues: write + Metadata: read |

For `copy`, the token needs write access to the target and read access to a private
source. A dry run can read public repositories without a token.

### Token storage and security

Login stores your token in a file named `token` in the [configuration directory](#configuration).
On Linux and macOS, only the owner can read or write the file (`0600`), inside an
owner-only directory (`0700`). On Windows, the file uses your user profile's
default permissions.

`auth status` reports the credential source without displaying the token.
`auth logout` removes the stored token; it does not unset environment variables.

## Configuration

Set preferences in `config.toml` in the labeldeck configuration directory:

```toml
prune = true
```

Pruning defaults to `false`. `--prune` or `--no-prune` overrides the config file
for one invocation. Unknown keys and malformed files cause an error.

| Platform | Configuration directory |
|----------|-------------------------|
| Linux | `~/.config/labeldeck` (`$XDG_CONFIG_HOME` honoured) |
| macOS | `~/Library/Application Support/labeldeck` |
| Windows | `%APPDATA%\labeldeck` |

Use `LABELDECK_CONFIG_DIR` to choose another directory. It holds `config.toml`,
the stored `token`, and the global `labels.json` deck.

Standard proxy environment variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`,
and `NO_PROXY`, upper or lower case) apply by default. Pass `--no-proxy` to bypass
proxies for one invocation.

Set `NO_COLOR` to disable colour output, including editor swatches.

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

Linux GNU builds require glibc 2.28 or newer. On older systems, the installer selects the static musl build.

The installer verifies the checksum and validates the downloaded binary before replacing an existing installation.

#### Unix installer (Linux/macOS)

```console
$ curl -fsSL https://raw.githubusercontent.com/seapagan/labeldeck/main/install.sh | sh
```

The installer selects a compatible binary and verifies its SHA-256 checksum. Set `LABELDECK_VERSION` to choose a release, or `LABELDECK_LIBC=gnu|musl` to override libc selection on Linux.

#### Windows installer (PowerShell)

```powershell
PS> Invoke-WebRequest https://raw.githubusercontent.com/seapagan/labeldeck/main/install.ps1 -OutFile install.ps1
PS> ./install.ps1
```

or, if your execution policy allows:

```powershell
PS> irm https://raw.githubusercontent.com/seapagan/labeldeck/main/install.ps1 | iex
```

The installer supports x64 and ARM64 and verifies the SHA-256 checksum. Set `LABELDECK_VERSION` to choose a release.

### cargo-binstall

```console
$ cargo binstall labeldeck
```

### From source

```console
$ cargo install --locked --git https://github.com/seapagan/labeldeck
```

Building from source requires Rust **1.88.0** or newer.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
