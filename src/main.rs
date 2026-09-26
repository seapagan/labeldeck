//! `labeldeck` binary entry point.

use clap::Parser;
use labeldeck::cli::{AuthCommand, Cli, Command};
use labeldeck::commands;

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Export {
            repo,
            file,
            force,
            global,
        } => commands::export::run(
            &repo,
            file.as_ref(),
            force,
            global,
            cli.no_proxy,
        ),
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
            repo,
            file,
            prune,
            no_prune,
            dry_run,
        } => commands::sync::run(
            &repo,
            file.as_ref(),
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
