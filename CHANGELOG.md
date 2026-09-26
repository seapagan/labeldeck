# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial implementation of `labeldeck`.
- `export OWNER/REPO [FILE]`: deterministic canonical JSON output (stdout
  or file, `--force` required to overwrite), works unauthenticated for
  public repositories.
- `diff FILE OWNER/REPO`: non-destructive comparison with
  `CREATE`/`UPDATE`/`DELETE`/`RETAIN`/`UNCHANGED` reporting and
  diff-style exit codes (0 clean, 1 differences, 2 errors).
- `sync FILE OWNER/REPO`: safe synchronization — creates missing labels,
  updates changed labels in place via GitHub's update API (never
  delete-and-recreate, preserving issue/PR associations), and deletes
  target-only labels only when pruning is enabled.
- Prune precedence: `--prune`/`--no-prune` CLI flags over `prune` in
  `config.toml` over the built-in default (off).
- `--dry-run` for `sync`: performs zero mutations.
- Defensive sync ordering: deletions never start until every create and
  update succeeded; partial failures report applied/failed/skipped
  accurately instead of claiming rollback.
- `auth login` (hidden interactive prompt or `--token-stdin` for
  scripts, validated against the API before storing), `auth logout`, and
  `auth status` (reports the token source without ever printing the
  token).
- Token resolution precedence: `LABELDECK_TOKEN` → `GH_TOKEN` →
  `GITHUB_TOKEN` → stored token → anonymous.
- Canonical file validation before any mutation: strict JSON schema
  (unknown keys rejected), duplicate names rejected under GitHub's
  case-insensitive uniqueness, colour normalisation to six lowercase hex
  digits, GitHub's observed name/description length limits.
- Link-header pagination (no assumption that a repository has fewer
  than 100 labels).
- Precise GitHub API error mapping for 401/403/404/422/429 including
  rate-limit reset hints and validation error codes.
- Cross-platform configuration directories (`~/.config/labeldeck`,
  `~/Library/Application Support/labeldeck`, `%APPDATA%\labeldeck`),
  overridable with `LABELDECK_CONFIG_DIR`.
- Unix token file written `0600` inside a `0700` directory; credentials
  kept out of `config.toml`.
- One-second pause between mutative requests per GitHub's
  secondary-rate-limit guidance.

[Unreleased]: https://github.com/seapagan/labeldeck/compare/v0.1.0...HEAD
