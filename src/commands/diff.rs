//! `labeldeck diff` — report what a sync would do, changing nothing.

use crate::commands::{
    github_client, read_canonical, remote_labels, repo_spec, resolve_token,
};
use crate::error::Result;
use crate::plan::{self, Plan};

pub fn run(
    repo: &str,
    file: Option<&std::path::PathBuf>,
    cli_prune: Option<bool>,
    no_proxy: bool,
) -> Result<i32> {
    let repo = repo_spec(repo)?;
    let path = file.map(std::path::PathBuf::as_path).unwrap_or_else(|| {
        std::path::Path::new(crate::cli::DEFAULT_LABELS_FILE)
    });
    let canonical = read_canonical(path)?;
    let config_dir = crate::commands::config_dir()?;
    let config = crate::config::load(&config_dir)?;
    let prune = crate::config::effective_prune(cli_prune, &config);
    let client = github_client(resolve_token(&config_dir).as_ref(), no_proxy);
    let remote = remote_labels(&client, &repo)?;

    let result = plan::plan(&canonical, &remote, prune);
    print_plan(&result);

    eprintln!("{}", crate::config::describe_prune(prune));
    eprintln!("{}", summarize(&result));

    // Exit code 1 (diff-style) when the repository differs from the
    // canonical file under the effective prune setting.
    Ok(i32::from(!result.is_empty()))
}

/// Render the plan as stable, greppable operation lines.
pub fn print_plan(result: &Plan) {
    for label in &result.creates {
        println!(
            "CREATE {} (color {}, description {})",
            label.name,
            label.color,
            quote(&label.description)
        );
    }
    for update in &result.updates {
        let current = &update.current;
        let desired = &update.desired;
        let mut changes = Vec::new();
        if current.color != desired.color {
            changes
                .push(format!("color {} -> {}", current.color, desired.color));
        }
        if current.description != desired.description {
            changes.push(format!(
                "description {} -> {}",
                quote(&current.description),
                quote(&desired.description)
            ));
        }
        println!("UPDATE {} ({})", current.name, changes.join(", "));
    }
    for deletion in &result.deletes {
        println!("DELETE {}", deletion.name);
    }
    for name in &result.retained {
        println!(
            "RETAIN {name} (target-only; kept because pruning is disabled)"
        );
    }
    for name in &result.unchanged {
        println!("UNCHANGED {name}");
    }
}

pub fn quote(text: &str) -> String {
    if text.is_empty() {
        "(none)".to_string()
    } else {
        format!("{text:?}")
    }
}

pub fn summarize(result: &Plan) -> String {
    format!(
        "{} create, {} update, {} delete, {} unchanged, {} retained",
        result.creates.len(),
        result.updates.len(),
        result.deletes.len(),
        result.unchanged.len(),
        result.retained.len(),
    )
}
