//! `labeldeck` binary entry point.

use std::path::Path;

use clap::Parser;
use labeldeck::cli::{AuthCommand, Cli, Command};
use labeldeck::commands;
use labeldeck::deck::ReadSource;

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Edit { repo, file, global } => commands::edit::run(
            repo.as_deref(),
            file.as_ref(),
            global,
            cli.no_proxy,
        ),
        Command::Export {
            interactive,
            repo,
            file,
            force,
            global,
        } => {
            let run = if interactive {
                commands::export::run_interactive
            } else {
                commands::export::run
            };
            run(&repo, file.as_ref(), force, global, cli.no_proxy)
        }
        Command::Diff {
            repo,
            file,
            global,
            prune,
            no_prune,
        } => commands::diff::run(
            &repo,
            read_source_from_cli(file.as_deref(), global),
            labeldeck::cli::prune_override(prune, no_prune),
            cli.no_proxy,
        ),
        Command::Sync {
            interactive,
            repo,
            file,
            global,
            prune,
            no_prune,
            dry_run,
        } => {
            let run = if interactive {
                commands::sync::run_interactive
            } else {
                commands::sync::run
            };
            run(
                &repo,
                read_source_from_cli(file.as_deref(), global),
                labeldeck::cli::prune_override(prune, no_prune),
                dry_run,
                cli.no_proxy,
            )
        }
        Command::Copy {
            interactive,
            source,
            target,
            prune,
            no_prune,
            dry_run,
        } => {
            let run = if interactive {
                commands::copy::run_interactive
            } else {
                commands::copy::run
            };
            run(
                &source,
                &target,
                labeldeck::cli::prune_override(prune, no_prune),
                dry_run,
                cli.no_proxy,
            )
        }
        Command::Auth(auth) => match auth {
            AuthCommand::Login { token_stdin } => {
                commands::auth::login(token_stdin, cli.no_proxy)
            }
            AuthCommand::Logout => commands::auth::logout(),
            AuthCommand::Status => commands::auth::status(),
        },
    };
    match code {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(error.exit_code());
        }
    }
}

fn read_source_from_cli(file: Option<&Path>, global: bool) -> ReadSource<'_> {
    match (file, global) {
        (Some(path), _) => ReadSource::File(path),
        (None, true) => ReadSource::Global,
        (None, false) => ReadSource::Auto,
    }
}
