//! Minimal synchronous GitHub REST client for repository labels.
//!
//! Talks directly to the documented v3 endpoints over `ureq` with rustls
//! (no system OpenSSL, no `gh` CLI). Non-2xx responses are handled here so
//! callers receive typed, actionable errors instead of raw statuses.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use ureq::http::header::HeaderValue;
use ureq::http::{Request, Response};
use ureq::middleware::{Middleware, MiddlewareNext};
use ureq::{Agent, Body, SendBody};

use crate::labels::{Label, LabelColor};

/// The pinned GitHub REST API version sent with every request.
pub const API_VERSION: &str = "2026-03-10";

const PER_PAGE: u32 = 100;
/// Safety valve against a server that always advertises a next page.
const MAX_PAGES: usize = 200;

const USER_AGENT: &str = concat!("labeldeck/", env!("CARGO_PKG_VERSION"));

/// An `OWNER/REPO` repository specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSpec {
    pub owner: String,
    pub name: String,
}

impl RepoSpec {
    /// Parse `OWNER/REPO`, rejecting anything with the wrong shape.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let parts: Vec<&str> = spec.split('/').collect();
        if parts.len() != 2 {
            return Err(format!(
                "invalid repository {spec:?}: expected the form OWNER/REPO \
                 (exactly one slash)"
            ));
        }
        let [owner, name] = [parts[0], parts[1]];
        if owner.is_empty() || name.is_empty() {
            return Err(format!(
                "invalid repository {spec:?}: owner and repository name \
                 must both be non-empty"
            ));
        }
        if spec.chars().any(char::is_whitespace) {
            return Err(format!(
                "invalid repository {spec:?}: whitespace is not allowed"
            ));
        }
        Ok(Self {
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }
}

/// Failures reported by the GitHub API layer.
#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    /// The HTTP/TLS request itself failed.
    #[error("could not reach GitHub: {0}")]
    Transport(String),
    /// HTTP 401: credentials missing, invalid, or expired.
    #[error("GitHub rejected the credentials (HTTP 401): {hint}")]
    Unauthorized { hint: String },
    /// HTTP 403 with an exhausted rate limit, or HTTP 429.
    #[error("GitHub rate limit exceeded ({detail})")]
    RateLimited { detail: String },
    /// HTTP 403 for reasons other than the rate limit.
    #[error("GitHub refused the request (HTTP 403): {detail}")]
    Forbidden { detail: String },
    /// HTTP 404.
    #[error(
        "{what} not found (HTTP 404); if the repository is private, \
             authentication is required"
    )]
    NotFound { what: String },
    /// HTTP 422: GitHub rejected the payload.
    #[error("GitHub rejected the change (HTTP 422): {detail}")]
    Validation { detail: String },
    /// Any other non-success status.
    #[error("unexpected GitHub response (HTTP {status}): {detail}")]
    Unexpected { status: u16, detail: String },
}

/// Adds the headers GitHub requires/recommends to every request.
struct GitHubHeaders(Option<Arc<str>>);

impl Middleware for GitHubHeaders {
    fn handle(
        &self,
        mut request: Request<SendBody>,
        next: MiddlewareNext,
    ) -> Result<Response<Body>, ureq::Error> {
        let headers = request.headers_mut();
        headers.insert(
            "Accept",
            HeaderValue::from_static("application/vnd.github+json"),
        );
        headers.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static(API_VERSION),
        );
        if let Some(token) = &self.0 {
            // The token only ever travels inside this header.
            if let Ok(value) =
                HeaderValue::from_str(&format!("Bearer {token}"))
            {
                headers.insert("Authorization", value);
            }
        }
        next.handle(request)
    }
}

/// A GitHub REST client for one authentication context.
#[derive(Clone)]
pub struct GitHubClient {
    agent: Agent,
    base_url: String,
}

impl GitHubClient {
    /// Client for the real GitHub API.
    pub fn new(token: Option<Arc<str>>) -> Self {
        Self::with_base_url("https://api.github.com", token)
    }

    /// Client for an explicit API base URL (used by tests to point at a
    /// local mock server).
    pub fn with_base_url(base_url: &str, token: Option<Arc<str>>) -> Self {
        let config = Agent::config_builder()
            .user_agent(USER_AGENT)
            // Non-2xx responses are inspected here, not converted to
            // errors, so bodies and rate-limit headers stay available.
            .http_status_as_error(false)
            // Proxy auto-detection from the environment is disabled so
            // behaviour never depends on ambient proxy variables.
            .proxy(None)
            .timeout_global(Some(Duration::from_secs(30)))
            .max_redirects(5)
            .middleware(GitHubHeaders(token))
            .build();
        let agent: Agent = config.into();
        Self {
            agent,
            base_url: base_url.to_string(),
        }
    }

    /// Resolve a request path against the base URL. Absolute URLs
    /// (GitHub's Link pagination headers) are used verbatim.
    fn url_for(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!("{}{}", self.base_url, path)
        }
    }

    fn get(&self, path: &str) -> Result<ApiReply, GithubError> {
        let url = self.url_for(path);
        let mut response =
            self.agent.get(&url).call().map_err(transport_error)?;
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(transport_error)?;
        Ok(ApiReply::new(response, body))
    }

    /// Perform a JSON-body request (POST or PATCH).
    fn send_json(
        &self,
        method: &str,
        path: &str,
        payload: &impl serde::Serialize,
    ) -> Result<ApiReply, GithubError> {
        let url = self.url_for(path);
        let mut response = match method {
            "POST" => self.agent.post(&url).send_json(payload),
            "PATCH" => self.agent.patch(&url).send_json(payload),
            _ => unreachable!("unsupported JSON method"),
        }
        .map_err(transport_error)?;
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(transport_error)?;
        Ok(ApiReply::new(response, body))
    }

    /// Perform a DELETE.
    fn delete(&self, path: &str) -> Result<ApiReply, GithubError> {
        let url = self.url_for(path);
        let mut response =
            self.agent.delete(&url).call().map_err(transport_error)?;
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(transport_error)?;
        Ok(ApiReply::new(response, body))
    }

    /// List every label in the repository, following `Link` pagination.
    pub fn list_labels(
        &self,
        repo: &RepoSpec,
    ) -> Result<Vec<Label>, GithubError> {
        let mut labels = Vec::new();
        let mut path = format!(
            "/repos/{}/{}/labels?per_page={PER_PAGE}",
            percent_encode(&repo.owner),
            percent_encode(&repo.name),
        );
        for _ in 0..MAX_PAGES {
            let reply = self.get(&path)?;
            if !reply.is_success() {
                return Err(reply.error(format!(
                    "repository {}/{}",
                    repo.owner, repo.name
                )));
            }
            let page: Vec<RemoteLabel> = serde_json::from_str(&reply.body)
                .map_err(|e| {
                    GithubError::Transport(format!(
                        "GitHub returned a malformed label list: {e}"
                    ))
                })?;
            labels.extend(page.into_iter().map(Label::from));
            // Follow the documented Link-header pagination rather than
            // constructing page URLs ourselves.
            match reply.next_page() {
                Some(next) => path = next,
                None => return Ok(labels),
            }
        }
        Err(GithubError::Transport(format!(
            "GitHub pagination exceeded {MAX_PAGES} pages; refusing to \
             loop forever"
        )))
    }

    /// Create a label. Success is HTTP 201.
    pub fn create_label(
        &self,
        repo: &RepoSpec,
        label: &Label,
    ) -> Result<(), GithubError> {
        let reply = self.send_json(
            "POST",
            &format!(
                "/repos/{}/{}/labels",
                percent_encode(&repo.owner),
                percent_encode(&repo.name)
            ),
            &CreateLabelBody {
                name: &label.name,
                color: label.color.as_str(),
                description: &label.description,
            },
        )?;
        if reply.is_success() {
            Ok(())
        } else {
            Err(reply.error(format!("creating label {:?}", label.name)))
        }
    }

    /// Update an existing label in place (colour/description only; the
    /// name is never renamed, so issue and pull-request associations are
    /// preserved).
    pub fn update_label(
        &self,
        repo: &RepoSpec,
        current_name: &str,
        desired: &Label,
    ) -> Result<(), GithubError> {
        let reply = self.send_json(
            "PATCH",
            &format!(
                "/repos/{}/{}/labels/{}",
                percent_encode(&repo.owner),
                percent_encode(&repo.name),
                percent_encode(current_name)
            ),
            &UpdateLabelBody {
                color: desired.color.as_str(),
                description: &desired.description,
            },
        )?;
        if reply.is_success() {
            Ok(())
        } else {
            Err(reply.error(format!("updating label {current_name:?}")))
        }
    }

    /// Delete a label. Success is HTTP 204.
    pub fn delete_label(
        &self,
        repo: &RepoSpec,
        name: &str,
    ) -> Result<(), GithubError> {
        let reply = self.delete(&format!(
            "/repos/{}/{}/labels/{}",
            percent_encode(&repo.owner),
            percent_encode(&repo.name),
            percent_encode(name)
        ))?;
        if reply.is_success() {
            Ok(())
        } else {
            Err(reply.error(format!("deleting label {name:?}")))
        }
    }

    /// Verify credentials by fetching the authenticated user's login.
    pub fn authenticated_login(&self) -> Result<String, GithubError> {
        let reply = self.get("/user")?;
        if !reply.is_success() {
            return Err(reply.error("the authenticated user".to_string()));
        }
        #[derive(Deserialize)]
        struct User {
            login: String,
        }
        let user: User = serde_json::from_str(&reply.body).map_err(|e| {
            GithubError::Transport(format!(
                "GitHub returned a malformed user response: {e}"
            ))
        })?;
        Ok(user.login)
    }
}

/// Status, interesting headers, and body of one API reply.
struct ApiReply {
    status: u16,
    rate_remaining: Option<u32>,
    rate_reset: Option<u64>,
    retry_after: Option<u64>,
    link: Option<String>,
    body: String,
}

impl ApiReply {
    fn new(response: Response<Body>, body: String) -> Self {
        let header = |name: &str| -> Option<String> {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        Self {
            status: response.status().as_u16(),
            rate_remaining: header("x-ratelimit-remaining")
                .and_then(|v| v.parse().ok()),
            rate_reset: header("x-ratelimit-reset")
                .and_then(|v| v.parse().ok()),
            retry_after: header("retry-after").and_then(|v| v.parse().ok()),
            link: header("link"),
            body,
        }
    }

    fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The URL of the `rel="next"` Link header entry, if present.
    fn next_page(&self) -> Option<String> {
        let link = self.link.as_deref()?;
        for entry in link.split(',') {
            let entry = entry.trim();
            let Some((url, params)) = entry.split_once(';') else {
                continue;
            };
            if !params.contains(r#"rel="next""#) {
                continue;
            }
            let url = url.trim();
            if url.starts_with('<') && url.ends_with('>') {
                return Some(url[1..url.len() - 1].to_string());
            }
        }
        None
    }

    /// Build the most specific error for a non-success reply.
    fn error(self, context: String) -> GithubError {
        let message = error_message(&self.body);
        match self.status {
            401 => GithubError::Unauthorized {
                hint: format!(
                    "{message}. Check the token with `labeldeck auth \
                     status`, or run `labeldeck auth login`"
                ),
            },
            403 if self.rate_remaining == Some(0) => {
                GithubError::RateLimited {
                    detail: format!(
                        "{message} ({})",
                        reset_hint(self.rate_reset)
                    ),
                }
            }
            403 => GithubError::Forbidden {
                detail: match self.retry_after {
                    Some(seconds) => format!(
                        "{message} (secondary rate limit; retry after \
                         {seconds}s)"
                    ),
                    None => message,
                },
            },
            404 => GithubError::NotFound { what: context },
            429 => GithubError::RateLimited {
                detail: match self.retry_after {
                    Some(seconds) => {
                        format!("{message} (retry after {seconds}s)")
                    }
                    None => message,
                },
            },
            422 => GithubError::Validation { detail: message },
            status => GithubError::Unexpected {
                status,
                detail: message,
            },
        }
    }
}

/// Extract GitHub's `message` plus any `errors[].code` entries (e.g.
/// `already_exists`) so validation failures say what actually failed.
fn error_message(body: &str) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let Some(value) = parsed else {
        return if body.trim().is_empty() {
            "no response body".to_string()
        } else {
            body.trim().to_string()
        };
    };
    let mut message = value
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap_or("no message in response body")
        .to_string();
    if let Some(codes) = value.get("errors").and_then(|e| e.as_array()) {
        let codes: Vec<String> = codes
            .iter()
            .filter_map(|entry| entry.get("code").and_then(|c| c.as_str()))
            .map(str::to_string)
            .collect();
        if !codes.is_empty() {
            message.push_str(&format!(" ({})", codes.join(", ")));
        }
    }
    message
}

fn reset_hint(reset_unix: Option<u64>) -> String {
    match reset_unix {
        Some(reset) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let minutes = reset.saturating_sub(now) / 60 + 1;
            format!("limit resets in about {minutes} minute(s)")
        }
        None => "unknown reset time".to_string(),
    }
}

fn transport_error(error: ureq::Error) -> GithubError {
    GithubError::Transport(error.to_string())
}

/// A label as returned by the GitHub API.
#[derive(Debug, Deserialize)]
struct RemoteLabel {
    name: String,
    color: LabelColor,
    description: Option<String>,
}

impl From<RemoteLabel> for Label {
    fn from(remote: RemoteLabel) -> Self {
        Self {
            name: remote.name,
            color: remote.color,
            description: remote.description.unwrap_or_default(),
        }
    }
}

/// The create request body: `name` plus colour and description.
#[derive(serde::Serialize)]
struct CreateLabelBody<'a> {
    name: &'a str,
    color: &'a str,
    description: &'a str,
}

/// The update request body: colour and description only.
///
/// Deliberately carries no `name`/`new_name` so existing labels are
/// modified in place and issue/pull-request associations are preserved.
#[derive(serde::Serialize)]
struct UpdateLabelBody<'a> {
    color: &'a str,
    description: &'a str,
}

/// Percent-encode a path segment (label names may contain spaces,
/// slashes, and non-ASCII characters).
fn percent_encode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~' => output.push(byte as char),
            _ => {
                let _ = write_percent(&mut output, byte);
            }
        }
    }
    output
}

fn write_percent(output: &mut String, byte: u8) -> std::fmt::Result {
    use std::fmt::Write;
    write!(output, "%{byte:02X}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_spec_parses_valid_input() {
        let spec = RepoSpec::parse("seapagan/labeldeck").unwrap();
        assert_eq!(spec.owner, "seapagan");
        assert_eq!(spec.name, "labeldeck");
    }

    #[test]
    fn repo_spec_rejects_invalid_input() {
        for input in [
            "",
            "owner",
            "owner/repo/extra",
            "/repo",
            "owner/",
            "owner repo",
            "a/b c",
        ] {
            assert!(RepoSpec::parse(input).is_err(), "input {input:?}");
        }
    }

    #[test]
    fn repo_spec_errors_are_actionable() {
        let err = RepoSpec::parse("owner/repo/extra").unwrap_err();
        assert!(err.contains("OWNER/REPO"), "{err}");
    }

    #[test]
    fn next_page_extracts_rel_next_url() {
        let mut reply = ApiReply {
            status: 200,
            rate_remaining: None,
            rate_reset: None,
            retry_after: None,
            link: Some(
                "<https://api.github.com/repositories/1/labels?per_page=100&page=2>; \
                 rel=\"next\", \
                 <https://api.github.com/repositories/1/labels?per_page=100&page=4>; \
                 rel=\"last\""
                    .to_string(),
            ),
            body: String::new(),
        };
        assert_eq!(
            reply.next_page().as_deref(),
            Some(
                "https://api.github.com/repositories/1/labels?per_page=100&page=2"
            )
        );
        reply.link = Some(
            "<https://api.github.com/repositories/1/labels?per_page=100&page=4>; \
             rel=\"last\""
                .to_string(),
        );
        assert_eq!(reply.next_page(), None);
        reply.link = None;
        assert_eq!(reply.next_page(), None);
    }

    #[test]
    fn percent_encode_keeps_unreserved_and_encodes_the_rest() {
        assert_eq!(percent_encode("simple-label_1.0~"), "simple-label_1.0~");
        assert_eq!(percent_encode("help wanted"), "help%20wanted");
        assert_eq!(percent_encode("a/b"), "a%2Fb");
        assert_eq!(percent_encode("café"), "caf%C3%A9");
        assert_eq!(percent_encode("50%"), "50%25");
    }

    #[test]
    fn error_mapping_covers_documented_statuses() {
        let reply =
            |status: u16, body: &str, remaining: Option<u32>| ApiReply {
                status,
                rate_remaining: remaining,
                rate_reset: None,
                retry_after: None,
                link: None,
                body: body.to_string(),
            };

        assert!(matches!(
            reply(401, r#"{"message":"Bad credentials"}"#, None)
                .error("ctx".into()),
            GithubError::Unauthorized { .. }
        ));
        assert!(matches!(
            reply(403, r#"{"message":"API rate limit"}"#, Some(0))
                .error("ctx".into()),
            GithubError::RateLimited { .. }
        ));
        assert!(matches!(
            reply(403, r#"{"message":"nope"}"#, Some(42)).error("ctx".into()),
            GithubError::Forbidden { .. }
        ));
        assert!(matches!(
            reply(404, "", None).error("repo x/y".into()),
            GithubError::NotFound { what } if what == "repo x/y"
        ));
        assert!(matches!(
            reply(
                422,
                r#"{"message":"Validation Failed","errors":[{"code":"already_exists"}]}"#,
                None
            )
            .error("ctx".into()),
            GithubError::Validation { detail } if detail.contains("Validation Failed")
        ));
        assert!(matches!(
            reply(500, "oops", None).error("ctx".into()),
            GithubError::Unexpected { status: 500, .. }
        ));
    }

    #[test]
    fn rate_limited_error_mentions_reset() {
        let reply = ApiReply {
            status: 403,
            rate_remaining: Some(0),
            rate_reset: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
                    + 600,
            ),
            retry_after: None,
            link: None,
            body: r#"{"message":"API rate limit exceeded"}"#.to_string(),
        };
        let error = reply.error("ctx".into());
        assert!(error.to_string().contains("resets in about"), "{error}");
    }
}
