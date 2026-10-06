//! `labeldeck` binary entry point.

use clap::Parser;
use labeldeck::cli::{AuthCommand, Cli, Command};
use labeldeck::commands;

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
            prune,
            no_prune,
        } => commands::diff::run(
            &repo,
            file.as_ref(),
            labeldeck::cli::prune_override(prune, no_prune),
            cli.no_proxy,
        ),
        Command::Sync {
            interactive,
            repo,
            file,
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
                file.as_ref(),
                labeldeck::cli::prune_override(prune, no_prune),
                dry_run,
                cli.no_proxy,
            )
        }
        Command::Copy {
            source,
            target,
            prune,
            no_prune,
            dry_run,
        } => commands::copy::run(
            &source,
            &target,
            labeldeck::cli::prune_override(prune, no_prune),
            dry_run,
            cli.no_proxy,
        ),
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
