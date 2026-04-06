use std::env;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use prism_core::AttestedHumanPrincipalInput;
use prism_ir::{
    HumanAttestationAssurance, HumanAttestationOperation, HumanAttestationRecord,
    PrincipalAuthorityId,
};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;

use crate::cli::AuthAssuranceArg;

const GITHUB_AUTHORITY_ID: &str = "github";
const GITHUB_DEVICE_FLOW_ISSUER: &str = "github-device-flow";
const GITHUB_OAUTH_CLIENT_ID_ENV: &str = "PRISM_GITHUB_OAUTH_CLIENT_ID";
const GITHUB_DEVICE_CODE_URL_ENV: &str = "PRISM_GITHUB_DEVICE_CODE_URL";
const GITHUB_ACCESS_TOKEN_URL_ENV: &str = "PRISM_GITHUB_ACCESS_TOKEN_URL";
const GITHUB_API_BASE_URL_ENV: &str = "PRISM_GITHUB_API_BASE_URL";
const DEFAULT_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const DEFAULT_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const DEFAULT_API_BASE_URL: &str = "https://api.github.com";
const GITHUB_DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
const GITHUB_READ_USER_SCOPE: &str = "read:user";

#[derive(Debug, Clone)]
pub(crate) struct VerifiedGithubIdentity {
    pub(crate) login: String,
    pub(crate) stable_subject: String,
}

pub(crate) fn issuer_uses_github_device_flow(issuer: &str) -> bool {
    issuer
        .trim()
        .eq_ignore_ascii_case(GITHUB_DEVICE_FLOW_ISSUER)
}

pub(crate) fn resolve_github_attested_human_input(
    operation: HumanAttestationOperation,
    authority: &str,
    expected_name: Option<&str>,
    role: Option<String>,
    expected_subject: Option<&str>,
    assurance: AuthAssuranceArg,
) -> Result<AttestedHumanPrincipalInput> {
    if assurance != AuthAssuranceArg::High {
        bail!("`github-device-flow` requires `--assurance high`");
    }
    let authority_id = resolve_github_authority(authority)?;
    let client_id = env::var(GITHUB_OAUTH_CLIENT_ID_ENV).with_context(|| {
        format!(
            "`{GITHUB_OAUTH_CLIENT_ID_ENV}` must be set to a GitHub OAuth app client id before using `github-device-flow`"
        )
    })?;
    let client = HttpGithubDeviceFlowClient::from_env()?;
    let verified = verify_github_device_flow(&client, &client_id)?;

    if let Some(expected_name) = sanitize_optional(expected_name) {
        if expected_name != verified.login {
            bail!(
                "verified GitHub login `{}` did not match requested `--name {expected_name}`",
                verified.login
            );
        }
    }
    if let Some(expected_subject) = sanitize_optional(expected_subject) {
        if expected_subject != verified.stable_subject {
            bail!(
                "verified GitHub subject `{}` did not match requested `--subject {expected_subject}`",
                verified.stable_subject
            );
        }
    }

    Ok(AttestedHumanPrincipalInput {
        authority_id: Some(authority_id),
        name: verified.login,
        role,
        attestation: HumanAttestationRecord {
            issuer: GITHUB_DEVICE_FLOW_ISSUER.to_string(),
            subject: verified.stable_subject,
            assurance: HumanAttestationAssurance::High,
            operation,
            verified_at: current_unix_timestamp(),
        },
    })
}

fn resolve_github_authority(authority: &str) -> Result<PrincipalAuthorityId> {
    let trimmed = authority.trim();
    if trimmed.is_empty() || trimmed == "local-daemon" || trimmed == GITHUB_AUTHORITY_ID {
        return Ok(PrincipalAuthorityId::new(GITHUB_AUTHORITY_ID));
    }
    bail!("`github-device-flow` must use authority `github`; got `{trimmed}`")
}

fn sanitize_optional(value: Option<&str>) -> Option<&str> {
    value.and_then(|candidate| {
        let trimmed = candidate.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

trait GithubDeviceFlowClient {
    fn request_device_code(&self, client_id: &str) -> Result<GithubDeviceCodeResponse>;
    fn poll_access_token(
        &self,
        client_id: &str,
        device_code: &str,
    ) -> Result<GithubAccessTokenPoll>;
    fn fetch_user(&self, access_token: &str) -> Result<GithubUserResponse>;
}

fn verify_github_device_flow(
    client: &dyn GithubDeviceFlowClient,
    client_id: &str,
) -> Result<VerifiedGithubIdentity> {
    let device = client.request_device_code(client_id)?;
    print_device_flow_instructions(&device);
    let mut interval_secs = device.interval.max(1);
    loop {
        match client.poll_access_token(client_id, &device.device_code)? {
            GithubAccessTokenPoll::Pending => {
                thread::sleep(Duration::from_secs(interval_secs));
            }
            GithubAccessTokenPoll::SlowDown => {
                interval_secs += 5;
                thread::sleep(Duration::from_secs(interval_secs));
            }
            GithubAccessTokenPoll::Denied => {
                bail!("GitHub device-flow authorization was denied");
            }
            GithubAccessTokenPoll::Expired => {
                bail!("GitHub device-flow authorization expired before it completed");
            }
            GithubAccessTokenPoll::Authorized { access_token } => {
                let user = client.fetch_user(&access_token)?;
                if user.login.trim().is_empty() {
                    bail!("GitHub returned an empty login for the authenticated user");
                }
                if user.id == 0 {
                    bail!("GitHub returned an invalid numeric user id");
                }
                return Ok(VerifiedGithubIdentity {
                    login: user.login,
                    stable_subject: user.id.to_string(),
                });
            }
        }
    }
}

fn print_device_flow_instructions(device: &GithubDeviceCodeResponse) {
    println!("Authenticate PRISM with GitHub");
    if let Some(uri_complete) = device.verification_uri_complete.as_deref() {
        println!("Open: {uri_complete}");
    } else {
        println!("Open: {}", device.verification_uri);
        println!("Code: {}", device.user_code);
    }
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("current time should be after unix epoch")
        .as_secs()
}

#[derive(Debug)]
struct HttpGithubDeviceFlowClient {
    client: Client,
    device_code_url: String,
    access_token_url: String,
    api_base_url: String,
}

impl HttpGithubDeviceFlowClient {
    fn from_env() -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&format!("prism-cli/{}", env!("CARGO_PKG_VERSION")))?,
        );
        let client = Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build GitHub attestation HTTP client")?;
        Ok(Self {
            client,
            device_code_url: env::var(GITHUB_DEVICE_CODE_URL_ENV)
                .unwrap_or_else(|_| DEFAULT_DEVICE_CODE_URL.to_string()),
            access_token_url: env::var(GITHUB_ACCESS_TOKEN_URL_ENV)
                .unwrap_or_else(|_| DEFAULT_ACCESS_TOKEN_URL.to_string()),
            api_base_url: env::var(GITHUB_API_BASE_URL_ENV)
                .unwrap_or_else(|_| DEFAULT_API_BASE_URL.to_string()),
        })
    }
}

impl GithubDeviceFlowClient for HttpGithubDeviceFlowClient {
    fn request_device_code(&self, client_id: &str) -> Result<GithubDeviceCodeResponse> {
        let response = self
            .client
            .post(&self.device_code_url)
            .form(&[("client_id", client_id), ("scope", GITHUB_READ_USER_SCOPE)])
            .send()
            .context("failed to request GitHub device code")?;
        let response = response
            .error_for_status()
            .context("GitHub device code request failed")?;
        response
            .json::<GithubDeviceCodeResponse>()
            .context("failed to parse GitHub device code response")
    }

    fn poll_access_token(
        &self,
        client_id: &str,
        device_code: &str,
    ) -> Result<GithubAccessTokenPoll> {
        let response = self
            .client
            .post(&self.access_token_url)
            .form(&[
                ("client_id", client_id),
                ("device_code", device_code),
                ("grant_type", GITHUB_DEVICE_GRANT_TYPE),
            ])
            .send()
            .context("failed to poll GitHub access token")?;
        let body = response
            .json::<GithubAccessTokenResponse>()
            .context("failed to parse GitHub access token response")?;
        match (body.access_token, body.error.as_deref()) {
            (Some(access_token), _) => Ok(GithubAccessTokenPoll::Authorized { access_token }),
            (None, Some("authorization_pending")) => Ok(GithubAccessTokenPoll::Pending),
            (None, Some("slow_down")) => Ok(GithubAccessTokenPoll::SlowDown),
            (None, Some("expired_token")) => Ok(GithubAccessTokenPoll::Expired),
            (None, Some("access_denied")) => Ok(GithubAccessTokenPoll::Denied),
            (None, Some(other)) => bail!("GitHub device-flow returned `{other}`"),
            (None, None) => Err(anyhow!(
                "GitHub device-flow response did not include an access token or error"
            )),
        }
    }

    fn fetch_user(&self, access_token: &str) -> Result<GithubUserResponse> {
        let response = self
            .client
            .get(format!("{}/user", self.api_base_url.trim_end_matches('/')))
            .header(AUTHORIZATION, format!("Bearer {access_token}"))
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .context("failed to fetch the authenticated GitHub user")?;
        let response = response
            .error_for_status()
            .context("GitHub user lookup failed")?;
        response
            .json::<GithubUserResponse>()
            .context("failed to parse GitHub user response")
    }
}

#[derive(Debug, Deserialize, Clone)]
struct GithubDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct GithubAccessTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

enum GithubAccessTokenPoll {
    Pending,
    SlowDown,
    Denied,
    Expired,
    Authorized { access_token: String },
}

#[derive(Debug, Deserialize, Clone)]
struct GithubUserResponse {
    id: u64,
    login: String,
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_github_attested_human_input, verify_github_device_flow, GithubAccessTokenPoll,
        GithubDeviceCodeResponse, GithubDeviceFlowClient, GithubUserResponse,
    };
    use crate::cli::AuthAssuranceArg;
    use anyhow::{anyhow, Result};
    use prism_ir::HumanAttestationOperation;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubGithubDeviceFlowClient {
        polls: Mutex<VecDeque<GithubAccessTokenPoll>>,
        user: Option<GithubUserResponse>,
    }

    impl GithubDeviceFlowClient for StubGithubDeviceFlowClient {
        fn request_device_code(&self, _client_id: &str) -> Result<GithubDeviceCodeResponse> {
            Ok(GithubDeviceCodeResponse {
                device_code: "device-code".to_string(),
                user_code: "ABCD-EFGH".to_string(),
                verification_uri: "https://github.com/login/device".to_string(),
                verification_uri_complete: Some(
                    "https://github.com/login/device?user_code=ABCD-EFGH".to_string(),
                ),
                interval: 0,
            })
        }

        fn poll_access_token(
            &self,
            _client_id: &str,
            _device_code: &str,
        ) -> Result<GithubAccessTokenPoll> {
            self.polls
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| anyhow!("no token poll response queued"))
        }

        fn fetch_user(&self, _access_token: &str) -> Result<GithubUserResponse> {
            self.user
                .clone()
                .ok_or_else(|| anyhow!("no GitHub user response queued"))
        }
    }

    #[test]
    fn verify_github_device_flow_returns_login_and_numeric_subject() {
        let client = StubGithubDeviceFlowClient {
            polls: Mutex::new(VecDeque::from([
                GithubAccessTokenPoll::Pending,
                GithubAccessTokenPoll::Authorized {
                    access_token: "token".to_string(),
                },
            ])),
            user: Some(GithubUserResponse {
                id: 123_456,
                login: "bene".to_string(),
            }),
        };

        let verified = verify_github_device_flow(&client, "client-id").unwrap();
        assert_eq!(verified.login, "bene");
        assert_eq!(verified.stable_subject, "123456");
    }

    #[test]
    fn github_device_flow_requires_high_assurance() {
        let result = resolve_github_attested_human_input(
            HumanAttestationOperation::Bootstrap,
            "github",
            None,
            None,
            None,
            AuthAssuranceArg::Moderate,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("requires `--assurance high`"));
    }
}
