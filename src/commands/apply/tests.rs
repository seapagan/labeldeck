use super::*;
#[test]
fn executing_an_empty_selected_plan_performs_no_network_work() {
    let client = GitHubClient::with_options("http://127.0.0.1:1", None, true);
    let target = RepoSpec::parse("o/r").unwrap();
    assert_eq!(
        execute_plan(
            &client,
            &target,
            &plan::Plan::default(),
            "Selected plan"
        )
        .unwrap(),
        0
    );
}
