//! Integration tests for the GitHub API client against a local mock
//! server. No test in this file contacts GitHub.

mod common;

use std::sync::Arc;

use common::{Expectation, labels_json, mock_github};
use labeldeck::github::{GitHubClient, GithubError, RepoSpec};
use labeldeck::labels::{Label, LabelColor};

fn repo() -> RepoSpec {
    RepoSpec::parse("octocat/hello-world").unwrap()
}

fn label(name: &str, color: &str, description: &str) -> Label {
    Label {
        name: name.to_string(),
        color: LabelColor::parse(color).unwrap(),
        description: description.to_string(),
    }
}

fn list_path() -> String {
    "/repos/octocat/hello-world/labels?per_page=100".to_string()
}

#[test]
fn list_labels_follows_link_pagination() {
    let mock = mock_github(Vec::new());
    let page_two = format!(
        "{}/repos/octocat/hello-world/labels?page=2",
        mock.base_url()
    );
    mock.expect(Expectation::get(&list_path()).labels_page(
        &labels_json(&[
            ("bug", "d73a4a", Some("broken")),
            ("docs", "0075ca", None),
        ]),
        Some(&page_two),
    ));
    mock.expect(
        Expectation::get("/repos/octocat/hello-world/labels?page=2")
            .labels_page(
                &labels_json(&[("feature", "a2eeef", Some("new"))]),
                None,
            ),
    );
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let labels = client.list_labels(&repo()).unwrap();
    mock.assert_satisfied();

    assert_eq!(labels.len(), 3);
    assert_eq!(labels[0].name, "bug");
    assert_eq!(labels[0].color.as_str(), "d73a4a");
    assert_eq!(labels[0].description, "broken");
    assert_eq!(labels[1].description, "");
}

#[test]
fn list_labels_single_page_has_no_follow_up() {
    let mock = mock_github(vec![
        Expectation::get(&list_path())
            .labels_page(&labels_json(&[("bug", "d73a4a", None)]), None),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let labels = client.list_labels(&repo()).unwrap();
    mock.assert_satisfied();
    assert_eq!(labels.len(), 1);
}

#[test]
fn list_labels_requires_only_one_request_when_empty() {
    let mock = mock_github(vec![
        Expectation::get(&list_path()).labels_page("[]", None),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    assert!(client.list_labels(&repo()).unwrap().is_empty());
    mock.assert_satisfied();
}

#[test]
fn unauthenticated_reads_send_no_authorization() {
    let mock = mock_github(vec![
        Expectation::get(&list_path()).labels_page("[]", None),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    client.list_labels(&repo()).unwrap();
    mock.assert_satisfied();

    let request = &mock.requests()[0];
    assert_eq!(request.header("authorization"), None);
}

#[test]
fn authenticated_requests_carry_bearer_and_documented_headers() {
    let mock = mock_github(vec![
        Expectation::get(&list_path()).labels_page("[]", None),
    ]);
    let token: Arc<str> = Arc::from("gh_test_token_ABC123");
    let client = GitHubClient::with_base_url(mock.base_url(), Some(token));
    client.list_labels(&repo()).unwrap();
    mock.assert_satisfied();

    let request = &mock.requests()[0];
    assert_eq!(
        request.header("authorization"),
        Some("Bearer gh_test_token_ABC123")
    );
    assert_eq!(
        request.header("accept"),
        Some("application/vnd.github+json")
    );
    assert_eq!(
        request.header("x-github-api-version"),
        Some(labeldeck::github::API_VERSION)
    );
    let user_agent = request.header("user-agent").unwrap();
    assert!(
        user_agent.starts_with("labeldeck/"),
        "unexpected user agent: {user_agent}"
    );
}

#[test]
fn create_label_sends_documented_body() {
    let mock = mock_github(vec![
        Expectation::post("/repos/octocat/hello-world/labels")
            .status(201)
            .body(&common::label_json("new", "00ff00", Some("fresh"))),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    client
        .create_label(&repo(), &label("new", "#00FF00", "fresh"))
        .unwrap();
    mock.assert_satisfied();

    let request = &mock.requests()[0];
    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["name"], "new");
    assert_eq!(body["color"], "00ff00");
    assert_eq!(body["description"], "fresh");
    assert_eq!(
        body.as_object().unwrap().keys().count(),
        3,
        "create body must carry exactly name/color/description"
    );
}

#[test]
fn update_label_patches_only_color_and_description() {
    let mock = mock_github(vec![
        Expectation::patch("/repos/octocat/hello-world/labels/bug")
            .body(&common::label_json("bug", "ff0000", Some("bad"))),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    client
        .update_label(&repo(), "bug", &label("bug", "ff0000", "bad"))
        .unwrap();
    mock.assert_satisfied();

    let request = &mock.requests()[0];
    let body: serde_json::Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["color"], "ff0000");
    assert_eq!(body["description"], "bad");
    assert!(
        body.get("name").is_none() && body.get("new_name").is_none(),
        "update must never rename: body was {}",
        request.body
    );
}

#[test]
fn label_names_are_percent_encoded_in_paths() {
    let mock = mock_github(vec![
        Expectation::patch("/repos/octocat/hello-world/labels/help%20wanted")
            .body(&common::label_json("help wanted", "00ff00", None)),
        Expectation::delete("/repos/octocat/hello-world/labels/a%2Fb")
            .status(204),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    client
        .update_label(
            &repo(),
            "help wanted",
            &label("help wanted", "00ff00", ""),
        )
        .unwrap();
    client.delete_label(&repo(), "a/b").unwrap();
    mock.assert_satisfied();
}

#[test]
fn delete_label_reports_missing_label() {
    let mock = mock_github(vec![
        Expectation::delete("/repos/octocat/hello-world/labels/gone")
            .status(404)
            .body(r#"{"message":"Not Found"}"#),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let error = client.delete_label(&repo(), "gone").unwrap_err();
    mock.assert_satisfied();
    assert!(matches!(error, GithubError::NotFound { .. }), "{error}");
}

#[test]
fn missing_repository_maps_to_not_found() {
    let mock = mock_github(vec![
        Expectation::get(&list_path())
            .status(404)
            .body(r#"{"message":"Not Found"}"#),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let error = client.list_labels(&repo()).unwrap_err();
    mock.assert_satisfied();
    let GithubError::NotFound { what } = error else {
        panic!("unexpected error: {error}");
    };
    assert!(what.contains("octocat/hello-world"), "{what}");
}

#[test]
fn bad_credentials_map_to_unauthorized_with_hint() {
    let mock = mock_github(vec![
        Expectation::get(&list_path())
            .status(401)
            .body(r#"{"message":"Bad credentials"}"#),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let error = client.list_labels(&repo()).unwrap_err();
    mock.assert_satisfied();
    let GithubError::Unauthorized { hint } = error else {
        panic!("unexpected error: {error}");
    };
    assert!(hint.contains("Bad credentials"), "{hint}");
    assert!(hint.contains("auth status"), "{hint}");
}

#[test]
fn exhausted_rate_limit_maps_to_rate_limited_with_reset() {
    let reset = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 900;
    let mock = mock_github(vec![
        Expectation::get(&list_path())
            .status(403)
            .header("x-ratelimit-remaining", "0")
            .header("x-ratelimit-reset", &reset.to_string())
            .body(r#"{"message":"API rate limit exceeded"}"#),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let error = client.list_labels(&repo()).unwrap_err();
    mock.assert_satisfied();
    assert!(matches!(error, GithubError::RateLimited { .. }), "{error}");
    assert!(error.to_string().contains("resets in about"), "{error}");
}

#[test]
fn too_many_requests_maps_to_rate_limited() {
    let mock = mock_github(vec![
        Expectation::get(&list_path())
            .status(429)
            .header("retry-after", "30")
            .body(r#"{"message":"You have exceeded a secondary rate limit"}"#),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let error = client.list_labels(&repo()).unwrap_err();
    mock.assert_satisfied();
    assert!(matches!(error, GithubError::RateLimited { .. }), "{error}");
}

#[test]
fn validation_failure_includes_error_codes() {
    let mock = mock_github(vec![
        Expectation::post("/repos/octocat/hello-world/labels")
            .status(422)
            .body(
                r#"{"message":"Validation Failed","errors":[
                    {"resource":"Label","code":"already_exists","field":"name"}]}"#,
            ),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let error = client
        .create_label(&repo(), &label("bug", "ff0000", ""))
        .unwrap_err();
    mock.assert_satisfied();
    let GithubError::Validation { detail } = error else {
        panic!("unexpected error: {error}");
    };
    assert!(detail.contains("already_exists"), "{detail}");
}

#[test]
fn forbidden_maps_to_forbidden() {
    let mock = mock_github(vec![
        Expectation::get(&list_path())
            .status(403)
            .header("x-ratelimit-remaining", "4999")
            .body(r#"{"message":"Resource not accessible by token"}"#),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let error = client.list_labels(&repo()).unwrap_err();
    mock.assert_satisfied();
    assert!(matches!(error, GithubError::Forbidden { .. }), "{error}");
}

#[test]
fn unexpected_status_maps_to_unexpected() {
    let mock = mock_github(vec![
        Expectation::get(&list_path())
            .status(502)
            .body("Bad gateway"),
    ]);
    let client = GitHubClient::with_base_url(mock.base_url(), None);
    let error = client.list_labels(&repo()).unwrap_err();
    mock.assert_satisfied();
    assert!(
        matches!(error, GithubError::Unexpected { status: 502, .. }),
        "{error}"
    );
}

#[test]
fn authenticated_login_returns_login_name() {
    let mock = mock_github(vec![
        Expectation::get("/user").body(r#"{"login":"seapagan"}"#),
    ]);
    let token: Arc<str> = Arc::from("gh_test_token_ABC123");
    let client = GitHubClient::with_base_url(mock.base_url(), Some(token));
    assert_eq!(client.authenticated_login().unwrap(), "seapagan");
    mock.assert_satisfied();
}

#[test]
fn transport_failure_is_reported_without_token_material() {
    // Point the client at a port with no listener.
    let client = GitHubClient::with_base_url(
        "http://127.0.0.1:1",
        Some(Arc::from("gh_test_token_ABC123")),
    );
    let error = client.list_labels(&repo()).unwrap_err();
    assert!(matches!(error, GithubError::Transport(_)), "{error}");
    assert!(
        !error.to_string().contains("gh_test_token"),
        "token leaked into transport error: {error}"
    );
}
