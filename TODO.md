# TODO

Deferred refactoring ideas recorded during the `copy` command review. These are future considerations, not known defects; the current behaviour is correct as shipped.

## Consider sharing read/write GitHub-client setup

`copy::client_for()` and the authentication/client construction in `sync` contain conceptually similar logic:

- dry-run/read-only mode may use an available token but does not require write credentials;
- mutating mode obtains credentials via the existing write-credential path;
- both construct the normal GitHub client with `no_proxy`.

`copy::client_for()` currently exists partly to keep `copy::run` below the configured cyclomatic-complexity threshold. A future focused refactor could determine whether this setup belongs in shared command plumbing. Do not force an abstraction merely to eliminate a few lines: the different command semantics and the current dry-run authentication behaviour must be preserved.

## Consider implementing `Display` for `RepoSpec`

Repository identity is rendered manually as `owner/name` in several places. A future cleanup could implement `Display` for `RepoSpec` if doing so genuinely simplifies diagnostics and output consistently across the application. This is convenience and consistency work, not a correctness issue.

## Consider a reusable case-insensitive repository-identity API

`copy` needs to know whether its source and target refer to the same GitHub repository, and correctly performs an ASCII-case-insensitive owner/name comparison (`is_same_repository` in `commands/copy.rs`). If more callers eventually need this, consider exposing an explicit semantic helper on `RepoSpec` — conceptually `same_repository_as(...)` or another clearly named identity comparison — so GitHub repository identity semantics are centralised. Do not change the derived `PartialEq` blindly: ordinary Rust equality should keep its current meaning throughout the codebase.

## Consider strongly typing the copy fetch role

`Error::LabelsFetch` stores `role: &'static str` with controlled internal values (`"source"` and `"target"`). A small internal enum could make invalid roles unrepresentable if this error path grows or gains more callers. The current implementation is not unsafe or incorrect; the values are controlled internally. This is a possible future robustness cleanup only.
