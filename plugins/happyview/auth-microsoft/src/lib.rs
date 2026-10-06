//! `happyview-auth-microsoft`: Microsoft account linking over OAuth 2.0, with the
//! profile read from Microsoft Graph.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};

use serde::Deserialize;

use happyview_plugin_sdk::host::{self, HostError, HttpRequest};
use happyview_plugin_sdk::{
    auth_plugin, serde_json, AuthorizeUrlInput, CallbackInput, ExternalProfile, PluginError,
    PluginInfo, RefreshInput, TokenInput, TokenSet,
};

const MS_AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const MS_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const GRAPH_PROFILE_URL: &str = "https://graph.microsoft.com/v1.0/me";
/// Fallback when the host supplies no `redirect_uri`.
const DEFAULT_REDIRECT_URI: &str = "http://localhost:3001/dashboard/settings/accounts/";

auth_plugin! {
    info: PluginInfo::new("happyview-auth-microsoft", "Microsoft", "0.1.0")
        .auth_type("oauth2")
        .required_secrets([
            "PLUGIN_HAPPYVIEW_AUTH_MICROSOFT_CLIENT_ID",
            "PLUGIN_HAPPYVIEW_AUTH_MICROSOFT_CLIENT_SECRET",
        ]),
    authorize_url: authorize_url,
    callback: callback,
    refresh: refresh,
    profile: profile,
}

#[derive(Deserialize)]
struct MsTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    #[allow(dead_code)]
    token_type: String,
}

#[derive(Deserialize)]
struct GraphUserProfile {
    id: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "userPrincipalName")]
    user_principal_name: Option<String>,
}

fn authorize_url(input: &AuthorizeUrlInput) -> Result<String, PluginError> {
    host::info("microsoft: get_authorize_url called");
    host::info(&format!(
        "microsoft: get_authorize_url redirect_uri={}",
        input.redirect_uri
    ));

    let Some(client_id) = host::get_secret("CLIENT_ID")? else {
        host::error("microsoft: CLIENT_ID not configured");
        return Err(PluginError::new("CONFIG_ERROR", "CLIENT_ID not configured"));
    };

    // Microsoft Graph scopes for user profile
    let scopes = "User.Read offline_access";
    let url = format!(
        "{}?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}",
        MS_AUTHORIZE_URL,
        urlencoding_encode(&client_id),
        urlencoding_encode(&input.redirect_uri),
        urlencoding_encode(scopes),
        urlencoding_encode(&input.state)
    );

    host::info("microsoft: get_authorize_url returning URL");
    Ok(url)
}

fn callback(input: &CallbackInput) -> Result<TokenSet, PluginError> {
    host::info("microsoft: handle_callback called");

    let Some(code) = input.param("code") else {
        host::error("microsoft: handle_callback no authorization code received");
        return Err(PluginError::new(
            "AUTH_FAILED",
            "No authorization code received",
        ));
    };

    host::info("microsoft: handle_callback received authorization code");

    let Some(client_id) = host::get_secret("CLIENT_ID")? else {
        host::error("microsoft: CLIENT_ID not configured");
        return Err(PluginError::new("CONFIG_ERROR", "CLIENT_ID not configured"));
    };

    let Some(client_secret) = host::get_secret("CLIENT_SECRET")? else {
        host::error("microsoft: CLIENT_SECRET not configured");
        return Err(PluginError::new(
            "CONFIG_ERROR",
            "CLIENT_SECRET not configured",
        ));
    };

    let redirect_uri = input.param("redirect_uri").unwrap_or(DEFAULT_REDIRECT_URI);

    host::info(&format!(
        "microsoft: handle_callback redirect_uri={}",
        redirect_uri
    ));

    host::info("microsoft: exchanging code for token");
    let body = format!(
        "client_id={}&client_secret={}&code={}&redirect_uri={}&grant_type=authorization_code",
        urlencoding_encode(&client_id),
        urlencoding_encode(&client_secret),
        urlencoding_encode(code),
        urlencoding_encode(redirect_uri)
    );

    let ms_token_resp = match http_post(MS_TOKEN_URL, &body, "application/x-www-form-urlencoded") {
        Ok(r) => r,
        Err(e) => {
            host::error(&format!("microsoft: token exchange failed: {}", e));
            return Err(
                PluginError::new("TOKEN_ERROR", format!("Failed to get token: {e}")).retryable(),
            );
        }
    };

    let ms_token: MsTokenResponse = match serde_json::from_str(&ms_token_resp) {
        Ok(t) => t,
        Err(e) => {
            host::error(&format!("microsoft: failed to parse token response: {}", e));
            return Err(PluginError::new(
                "TOKEN_ERROR",
                format!("Failed to parse token: {e}"),
            ));
        }
    };

    host::info("microsoft: handle_callback token exchange successful");
    Ok(token_set(ms_token))
}

fn refresh(input: &RefreshInput) -> Result<TokenSet, PluginError> {
    host::info("microsoft: refresh_tokens called");

    let Some(client_id) = host::get_secret("CLIENT_ID")? else {
        host::error("microsoft: CLIENT_ID not configured");
        return Err(PluginError::new("CONFIG_ERROR", "CLIENT_ID not configured"));
    };

    let Some(client_secret) = host::get_secret("CLIENT_SECRET")? else {
        host::error("microsoft: CLIENT_SECRET not configured");
        return Err(PluginError::new(
            "CONFIG_ERROR",
            "CLIENT_SECRET not configured",
        ));
    };

    host::info("microsoft: refreshing token");
    let body = format!(
        "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
        urlencoding_encode(&client_id),
        urlencoding_encode(&client_secret),
        urlencoding_encode(&input.refresh_token)
    );

    let resp = match http_post(MS_TOKEN_URL, &body, "application/x-www-form-urlencoded") {
        Ok(r) => r,
        Err(e) => {
            host::error(&format!("microsoft: token refresh failed: {}", e));
            return Err(
                PluginError::new("TOKEN_ERROR", format!("Failed to refresh: {e}")).retryable(),
            );
        }
    };

    let ms_token: MsTokenResponse = match serde_json::from_str(&resp) {
        Ok(t) => t,
        Err(e) => {
            host::error(&format!(
                "microsoft: failed to parse refresh response: {}",
                e
            ));
            return Err(PluginError::new(
                "TOKEN_ERROR",
                format!("Failed to parse: {e}"),
            ));
        }
    };

    host::info("microsoft: refresh_tokens successful");
    Ok(token_set(ms_token))
}

fn profile(input: &TokenInput) -> Result<ExternalProfile, PluginError> {
    host::info("microsoft: get_profile called");
    host::info("microsoft: fetching profile from Graph API");

    let graph_profile = match http_get_with_auth(GRAPH_PROFILE_URL, &input.access_token) {
        Ok(resp) => match serde_json::from_str::<GraphUserProfile>(&resp) {
            Ok(p) => p,
            Err(e) => {
                host::error(&format!("microsoft: failed to parse profile: {}", e));
                return Err(PluginError::new(
                    "PROFILE_ERROR",
                    format!("Failed to parse profile: {e}"),
                ));
            }
        },
        Err(e) => {
            host::error(&format!("microsoft: failed to get profile: {}", e));
            return Err(
                PluginError::new("PROFILE_ERROR", format!("Failed to get profile: {e}"))
                    .retryable(),
            );
        }
    };

    host::info(&format!(
        "microsoft: get_profile successful for id={}",
        graph_profile.id
    ));

    Ok(ExternalProfile {
        account_id: graph_profile.id,
        display_name: graph_profile.display_name,
        profile_url: Some("https://account.microsoft.com/profile".to_string()),
        // Microsoft Graph requires a separate call for the photo.
        avatar_url: None,
    })
}

fn token_set(ms_token: MsTokenResponse) -> TokenSet {
    let mut tokens = TokenSet::new(ms_token.access_token, "Bearer");
    tokens.refresh_token = ms_token.refresh_token;
    if let Some(secs) = ms_token.expires_in {
        tokens = tokens.expires_in(secs);
    }
    tokens
}

fn http_post(url: &str, body: &str, content_type: &str) -> Result<String, String> {
    let response = host::http_request(
        &HttpRequest::new("POST", url)
            .header("Content-Type", content_type)
            .body_text(body),
    )
    .map_err(host_error)?;
    if (200..300).contains(&response.status) {
        Ok(response.text().into_owned())
    } else {
        Err(format!("HTTP {}: {}", response.status, response.text()))
    }
}

fn http_get_with_auth(url: &str, token: &str) -> Result<String, String> {
    let response = host::http_request(
        &HttpRequest::new("GET", url).header("Authorization", format!("Bearer {token}")),
    )
    .map_err(host_error)?;
    if (200..300).contains(&response.status) {
        Ok(response.text().into_owned())
    } else {
        Err(format!("HTTP {}", response.status))
    }
}

/// The message reported when a host call returns no response.
fn host_error(err: HostError) -> String {
    match err {
        HostError::Plugin(err) => err.message,
        HostError::NoResponse => "No response".to_string(),
        HostError::NotWasm => "host functions are unavailable outside wasm32".to_string(),
    }
}

fn urlencoding_encode(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            _ => {
                for b in c.to_string().as_bytes() {
                    result.push('%');
                    result.push_str(&format!("{:02X}", b));
                }
            }
        }
    }
    result
}
