use std::time::Duration;

use clipanion::cli;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use zpm_parsers::{DataDocument, JsonDocument};
use zpm_utils::{DataType, IoResultExt, QueryString};

use crate::{
    error::Error,
    http::HttpClient,
    http_npm::{self, get_registry, NpmHttpParams},
    project::Project,
    report::{current_report, with_report_result, PromptType, StreamReport, StreamReportConfig},
};

const WEB_LOGIN_REGISTRIES: &[&str] = &[
    "https://registry.npmjs.org",
    "https://registry.yarnpkg.com",
];

/// Store credentials for an npm registry
///
/// This command authenticates with a registry and stores the generated token in the home configuration file. It never writes credentials to the
/// project configuration.
///
/// Use `-s,--scope` to authenticate against the registry configured for a scope.
///
/// Use `--publish` to authenticate against the registry used for publishing.
///
#[cli::command]
#[cli::path("npm", "login")]
#[cli::category("Npm-related commands")]
pub struct Login {
    /// Authenticate with the registry configured for this scope
    #[cli::option("-s,--scope")]
    scope: Option<String>,

    /// Authenticate with the publish registry
    #[cli::option("--publish", default = false)]
    publish: bool,

    /// Enable or disable web-based login
    #[cli::option("--web-login")]
    web_login: Option<bool>,

    /// Store credentials with `npmAlwaysAuth` enabled
    #[cli::option("--always-auth", default = false)]
    always_auth: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NpmLoginPayload {
    #[serde(rename = "_id")]
    id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "_rev")]
    rev: Option<String>,
    name: String,
    password: String,
    user_type: String,
    roles: Vec<String>,
    date: String,
}

#[derive(Deserialize)]
struct NpmLoginResponse {
    token: String,
    #[allow(dead_code)]
    ok: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NpmWebLoginInitResponse {
    login_url: String,
    done_url: String,
}

struct NpmWebLoginInit {
    response: NpmWebLoginInitResponse,
    notices: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NpmWebLoginCheckResponse {
    token: String,
}

enum NpmWebLoginState {
    Waiting(Duration),
    Done(String),
}

struct Credentials {
    username: String,
    password: String,
}

impl Login {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let registry
            = get_registry(&project.config, self.scope.as_deref(), self.publish)?
                .to_string();

        let report = StreamReport::new(StreamReportConfig {
            ..StreamReportConfig::from_config(&project.config)
        });

        with_report_result(report, async {
            crate::report::if_active_async(|report| {
                report.info(format!("Logging in to {}", DataType::Url.colorize(&registry)));
            }).await;

            let token
                = self.authenticate(&project.http_client, &registry).await?;

            let Some(config_path) = project.config.user_config_path else {
                return Err(Error::AuthenticationError("Failed to get user config path".to_string()));
            };

            let config_content = config_path
                .fs_read_text()
                .ok_missing()?
                .unwrap_or_default();

            let auth_token_path = npm_credential_path(self.scope.as_deref(), &registry, "npmAuthToken");

            let mut updated_content = DataDocument::update_document_field(
                &config_content,
                auth_token_path,
                zpm_parsers::Value::String(token),
            )?;

            if self.always_auth {
                let always_auth_path = npm_credential_path(self.scope.as_deref(), &registry, "npmAlwaysAuth");

                updated_content = DataDocument::update_document_field(
                    &updated_content,
                    always_auth_path,
                    zpm_parsers::Value::Bool(true),
                )?;
            }

            config_path
                .fs_write_text(&updated_content)?;

            crate::report::if_active_async(|report| {
                report.info("Successfully logged in".to_string());
            }).await;

            Ok(())
        }).await
    }

    async fn authenticate(&self, http_client: &HttpClient, registry: &str) -> Result<String, Error> {
        let enable_web_login
            = self.web_login
                .unwrap_or_else(|| WEB_LOGIN_REGISTRIES.contains(&registry));

        if enable_web_login {
            let token = self.login_via_web(
                &http_client,
                &registry,
            ).await?;

            if let Some(token) = token {
                return Ok(token);
            }
        }

        let is_github
            = registry.contains("npm.pkg.github.com");

        if is_github {
            println!("Note: You seem to be using the GitHub Package Registry.");
            println!("Tokens must be generated with the 'repo', 'write:packages', and 'read:packages' permissions.");
        }

        let credentials
            = get_credentials(is_github).await?;

        // Authenticate with registry
        let token = self.register_or_login(
            &http_client,
            &registry,
            &credentials.username,
            &credentials.password,
        ).await?;

        Ok(token)
    }

    async fn web_login_init(&self, http_client: &HttpClient, registry: &str) -> Result<Option<NpmWebLoginInit>, Error> {
        let response = http_client
            .post(format!("{}/-/v1/login", registry))?
            .header("npm-auth-type", Some("web"))
            .enable_status_check(false)
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(None);
        }

        // The headers must be read before `text()` consumes the response.
        // Invalid UTF-8 is decoded lossily so that a malformed notice cannot fail the login.
        let notices
            = response.headers()
                .get_all("npm-notice")
                .iter()
                .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
                .collect();

        let login_body
            = response.text().await
                .map_err(|e| Error::AuthenticationError(format!("Failed to read response: {}", e)))?;

        let response
            = JsonDocument::hydrate_from_str(&login_body)
                .map_err(|e| Error::AuthenticationError(format!("Failed to parse response: {}", e)))?;

        Ok(Some(NpmWebLoginInit {
            response,
            notices,
        }))
    }

    async fn web_login_check(&self, http_client: &HttpClient, done_url: &str) -> Result<NpmWebLoginState, Error> {
        let response = http_client
            .get(done_url)?
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::AuthenticationError(format!(
                "Failed to retrieve the login token: {}",
                response.status()
            )));
        }

        if response.status() == StatusCode::ACCEPTED {
            let retry_duration
                = response.headers().get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(1);

            return Ok(NpmWebLoginState::Waiting(Duration::from_secs(retry_duration)));
        }

        let done_body
            = response.text().await
                .map_err(|e| Error::AuthenticationError(format!("Failed to read response: {}", e)))?;

        let done_response: NpmWebLoginCheckResponse
            = JsonDocument::hydrate_from_str(&done_body)
                .map_err(|e| Error::AuthenticationError(format!("Failed to parse response: {}", e)))?;

        Ok(NpmWebLoginState::Done(done_response.token))
    }

    async fn login_via_web(&self, http_client: &HttpClient, registry: &str) -> Result<Option<String>, Error> {
        let Some(init) = self.web_login_init(http_client, registry).await? else {
            return Ok(None);
        };

        let login_response
            = init.response;

        // Registries use the notices to show a verification code, so they must be visible before the browser opens.
        crate::report::if_active_async(|report| {
            for notice in &init.notices {
                let notice
                    = sanitize_notice(notice);

                if !notice.is_empty() {
                    report.info(notice);
                }
            }
        }).await;

        if std::env::var("YARN_IS_TEST_ENV").is_err() {
            open::that(login_response.login_url)?;
        }

        loop {
            let check
                = self.web_login_check(http_client, login_response.done_url.as_str()).await?;

            match check {
                NpmWebLoginState::Waiting(duration) => {
                    sleep(duration).await;
                },

                NpmWebLoginState::Done(token) => {
                    return Ok(Some(token));
                },
            }
        }
    }

    async fn register_or_login(&self, http_client: &HttpClient, registry: &str, username: &str, password: &str) -> Result<String, Error> {
        let user_id
            = format!("org.couchdb.user:{}", QueryString::encode(username));
        let user_url
            = format!("/-/user/{}", user_id);

        let payload = JsonDocument::to_string(&NpmLoginPayload {
            id: format!("org.couchdb.user:{}", username),
            name: username.to_string(),
            password: password.to_string(),
            user_type: "user".to_string(),
            roles: vec![],
            date: chrono::Utc::now().to_rfc3339(),
            rev: None,
        })?;

        let response = http_npm::put(&NpmHttpParams {
            http_client,
            registry,
            path: user_url.as_str(),
            authorization: None,
            otp: None,
        }, payload).await
            .map_err(|err| match err {
                Error::AuthenticationError(message) if message.starts_with("Invalid authentication") =>
                    Error::AuthenticationError(format!("Invalid authentication (attempted as {})", username)),
                other => other,
            })?;

        let body
            = response.text().await
                .map_err(|e| Error::AuthenticationError(format!("Failed to read response: {}", e)))?;

        let login_response: NpmLoginResponse
            = JsonDocument::hydrate_from_str(&body)
                .map_err(|e| Error::AuthenticationError(format!("Failed to parse response: {}", e)))?;

        return Ok(login_response.token);
    }
}

async fn get_credentials(is_token: bool) -> Result<Credentials, Error> {
    if std::env::var("YARN_IS_TEST_ENV").is_ok() {
        return Ok(Credentials {
            username: std::env::var("YARN_INJECT_NPM_USER").unwrap_or_default(),
            password: std::env::var("YARN_INJECT_NPM_PASSWORD").unwrap_or_default(),
        });
    }

    let report_guard
        = current_report().await;

    let report
        = report_guard.as_ref()
            .expect("No report set");

    let username
        = report.prompt(PromptType::Input("Username".to_string())).await;

    let password
        = report.prompt(PromptType::Password(if is_token {"Token"} else {"Password"}.to_string())).await;

    Ok(Credentials {
        username,
        password,
    })
}

/// Builds the rc-document path for an npm credential key (`leaf`), keyed
/// by scope when present, otherwise by registry URL.
fn npm_credential_path(scope: Option<&str>, registry: &str, leaf: &str) -> zpm_parsers::Path {
    match scope {
        Some(scope) => zpm_parsers::Path::from_segments(vec![
            "npmScopes".to_string(),
            scope.to_string(),
            leaf.to_string(),
        ]),
        None => zpm_parsers::Path::from_segments(vec![
            "npmRegistries".to_string(),
            registry.to_string(),
            leaf.to_string(),
        ]),
    }
}

/// Removes escape sequences, control characters, and invisible formatting
/// characters (such as bidi overrides) from text sent by a registry, so that
/// it cannot alter what the terminal displays.
fn sanitize_notice(notice: &str) -> String {
    let mut output
        = String::with_capacity(notice.len());

    let mut chars
        = notice.chars().peekable();

    let mut pending_break
        = false;

    while let Some(ch) = chars.next() {
        match ch {
            // OSC: a string terminated by BEL or ST
            '\u{9d}' => {
                skip_string_sequence(&mut chars);
            },

            '\u{1b}' if chars.next_if_eq(&']').is_some() => {
                skip_string_sequence(&mut chars);
            },

            // CSI: parameter and intermediate bytes, then a final byte
            '\u{9b}' => {
                skip_csi_sequence(&mut chars);
            },

            '\u{1b}' if chars.next_if_eq(&'[').is_some() => {
                skip_csi_sequence(&mut chars);
            },

            // Any other escape sequence is the escape character and one more character
            '\u{1b}' => {
                chars.next();
            },

            // Line breaks and tabs become a single space so that words do not run together
            '\n' | '\r' | '\t' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                pending_break = true;
            },

            ch if ch.is_control() || is_formatting_char(ch) => {},

            ch => {
                // The break only adds a space when no space is next to it already
                if pending_break && ch != ' ' && !output.is_empty() && !output.ends_with(' ') {
                    output.push(' ');
                }

                pending_break = false;
                output.push(ch);
            },
        }
    }

    output.trim().to_string()
}

fn skip_csi_sequence(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while chars.next_if(|c| ('\u{20}'..='\u{3f}').contains(c)).is_some() {}

    chars.next_if(|c| ('\u{40}'..='\u{7e}').contains(c));
}

fn skip_string_sequence(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(ch) = chars.next() {
        match ch {
            '\u{7}' | '\u{9c}' => break,

            '\u{1b}' if chars.peek() == Some(&'\\') => {
                chars.next();
                break;
            },

            _ => {},
        }
    }
}

fn is_formatting_char(ch: char) -> bool {
    matches!(ch,
        '\u{ad}'
        | '\u{600}'..='\u{605}'
        | '\u{61c}'
        | '\u{6dd}'
        | '\u{70f}'
        | '\u{180e}'
        | '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206f}'
        | '\u{feff}'
        | '\u{fff9}'..='\u{fffb}'
        | '\u{e0001}'
        | '\u{e0020}'..='\u{e007f}'
    )
}

#[cfg(test)]
mod tests {
    use super::sanitize_notice;

    #[test]
    fn keeps_plain_text() {
        assert_eq!(
            sanitize_notice("Verification code: 123456. Enter this code in the browser to complete the login."),
            "Verification code: 123456. Enter this code in the browser to complete the login.",
        );
    }

    #[test]
    fn strips_escape_sequences() {
        assert_eq!(sanitize_notice("\u{1b}[31mCode\u{1b}[0m: 123456"), "Code: 123456");
        assert_eq!(sanitize_notice("\u{9b}31mCode\u{9b}0m: 123456"), "Code: 123456");
        assert_eq!(sanitize_notice("A\u{1b}]0;title\u{7}B"), "AB");
        assert_eq!(sanitize_notice("A\u{1b}]8;;https://example.com\u{1b}\\B"), "AB");
        assert_eq!(sanitize_notice("A\u{1b}cB"), "AB");
    }

    #[test]
    fn strips_control_and_formatting_characters() {
        assert_eq!(sanitize_notice("Code:\t\u{202e}123\u{200b}456\u{85}\r\n"), "Code: 123456");
    }

    #[test]
    fn turns_line_breaks_into_single_spaces() {
        assert_eq!(sanitize_notice("Line one\nLine two"), "Line one Line two");
        assert_eq!(sanitize_notice("Line one\r\n\r\nLine two\u{2028}Line three"), "Line one Line two Line three");
        assert_eq!(sanitize_notice("Code:\t123"), "Code: 123");
        assert_eq!(sanitize_notice("\n Code \t\n"), "Code");
    }

    #[test]
    fn does_not_add_a_space_next_to_a_space() {
        assert_eq!(sanitize_notice("a\n b"), "a b");
        // Literal spaces stay as sent; the line break adds none
        assert_eq!(sanitize_notice("a \n b"), "a  b");
        assert_eq!(sanitize_notice("Line one\n  Line two"), "Line one  Line two");
        assert_eq!(sanitize_notice("a\u{2028}\u{200b}\n b"), "a b");
        assert_eq!(sanitize_notice("a \nb"), "a b");
    }

    #[test]
    fn treats_next_line_vertical_tab_and_form_feed_as_breaks() {
        assert_eq!(sanitize_notice("a\u{85}b"), "a b");
        assert_eq!(sanitize_notice("a\u{b}b"), "a b");
        assert_eq!(sanitize_notice("a\u{c}b"), "a b");
    }

    #[test]
    fn is_idempotent() {
        for input in [
            "a\n b",
            "a \n b",
            "Line one\n  Line two",
            "a\u{2028}\u{200b}\n b",
            "a\u{85}b",
            "a\u{b}b",
            "a\u{c}b",
            "Line one\nLine two",
            "Code:\t123",
            "\n Code \t\n",
        ] {
            let once = sanitize_notice(input);

            assert_eq!(sanitize_notice(&once), once, "input: {input:?}");
        }
    }

    #[test]
    fn empty_after_stripping() {
        assert_eq!(sanitize_notice("\u{1b}[31m\u{200b} \t\n"), "");
    }
}
