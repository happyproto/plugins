//! `happyview-auth-itch`: itch.io account linking over OAuth 2.0.

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

const ITCH_AUTHORIZE_URL: &str = "https://itch.io/user/oauth";
const ITCH_TOKEN_URL: &str = "https://itch.io/api/1/oauth/token";
const ITCH_API_BASE: &str = "https://itch.io/api/1";

auth_plugin! {
    info: PluginInfo::new("happyview-auth-itch", "itch.io", "0.1.0")
        .icon_url("https://itch.io/favicon.ico")
        .auth_type("oauth2")
        .required_secrets([
            "PLUGIN_HAPPYVIEW_AUTH_ITCH_CLIENT_ID",
            "PLUGIN_HAPPYVIEW_AUTH_ITCH_CLIENT_SECRET",
        ]),
    authorize_url: authorize_url,
    callback: callback,
    refresh: refresh,
    profile: profile,
}

#[derive(Deserialize)]
struct ItchTokenResponse {
    access_token: String,
    token_type: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct ItchMeResponse {
    user: ItchUser,
}

#[derive(Deserialize)]
struct ItchUser {
    id: i64,
    username: String,
    display_name: Option<String>,
    cover_url: Option<String>,
    url: Option<String>,
}

fn authorize_url(input: &AuthorizeUrlInput) -> Result<String, PluginError> {
    host::info("itch: get_authorize_url called");
    host::info(&format!(
        "itch: get_authorize_url redirect_uri={}",
        input.redirect_uri
    ));

    let Some(client_id) = host::get_secret("CLIENT_ID")? else {
        host::error("itch: CLIENT_ID not configured");
        return Err(PluginError::new(
            "MISSING_SECRET",
            "CLIENT_ID not configured",
        ));
    };

    // itch.io OAuth2 scopes: profile:me, profile:games (owned games)
    let scopes = "profile:me";

    let url = format!(
        "{}?client_id={}&scope={}&response_type=code&redirect_uri={}&state={}",
        ITCH_AUTHORIZE_URL,
        urlencod(&client_id),
        urlencod(scopes),
        urlencod(&input.redirect_uri),
        urlencod(&input.state)
    );

    host::info("itch: get_authorize_url returning URL");
    Ok(url)
}

fn callback(input: &CallbackInput) -> Result<TokenSet, PluginError> {
    host::info("itch: handle_callback called");

    let Some(code) = input.param("code") else {
        host::error("itch: handle_callback no authorization code received");
        return Err(PluginError::new(
            "MISSING_CODE",
            "Authorization code is required",
        ));
    };

    host::info("itch: handle_callback received authorization code");

    let Some(client_id) = host::get_secret("CLIENT_ID")? else {
        host::error("itch: CLIENT_ID not configured");
        return Err(PluginError::new(
            "MISSING_SECRET",
            "CLIENT_ID not configured",
        ));
    };

    let Some(client_secret) = host::get_secret("CLIENT_SECRET")? else {
        host::error("itch: CLIENT_SECRET not configured");
        return Err(PluginError::new(
            "MISSING_SECRET",
            "CLIENT_SECRET not configured",
        ));
    };

    host::info("itch: exchanging code for token");
    let token_body = format!(
        "grant_type=authorization_code&code={}&client_id={}&client_secret={}",
        urlencod(code),
        urlencod(&client_id),
        urlencod(&client_secret)
    );

    let token_response = match http_post(
        ITCH_TOKEN_URL,
        &token_body,
        "application/x-www-form-urlencoded",
    ) {
        Ok(r) => r,
        Err(e) => {
            host::error(&format!("itch: token exchange failed: {}", e));
            return Err(
                PluginError::new("TOKEN_ERROR", format!("Token exchange failed: {e}")).retryable(),
            );
        }
    };

    let tokens: ItchTokenResponse = match serde_json::from_str(&token_response) {
        Ok(t) => t,
        Err(e) => {
            host::error(&format!("itch: failed to parse token response: {}", e));
            return Err(PluginError::new(
                "INVALID_RESPONSE",
                format!("Failed to parse token: {e}"),
            ));
        }
    };

    host::info("itch: handle_callback token exchange successful");
    // itch.io tokens do not expire, so no expiry is set.
    let mut token_set = TokenSet::new(tokens.access_token, tokens.token_type);
    token_set.refresh_token = tokens.refresh_token;
    Ok(token_set)
}

/// itch.io tokens do not expire, so this hands the same one straight back.
fn refresh(input: &RefreshInput) -> Result<TokenSet, PluginError> {
    host::info("itch: refresh_tokens called (itch.io tokens don't expire)");
    host::info("itch: refresh_tokens returning same token (no expiry)");
    Ok(TokenSet::new(input.refresh_token.clone(), "bearer"))
}

fn profile(input: &TokenInput) -> Result<ExternalProfile, PluginError> {
    host::info("itch: get_profile called");
    host::info("itch: fetching user profile from /me");

    let url = format!("{}/me", ITCH_API_BASE);
    let body = match http_get_with_auth(&url, &input.access_token) {
        Ok(b) => b,
        Err(e) => {
            host::error(&format!("itch: get_profile HTTP error: {}", e));
            return Err(PluginError::new("HTTP_ERROR", e).retryable());
        }
    };

    let me: ItchMeResponse = match serde_json::from_str(&body) {
        Ok(m) => m,
        Err(e) => {
            host::error(&format!("itch: failed to parse profile response: {}", e));
            return Err(PluginError::new(
                "INVALID_RESPONSE",
                format!("Parse error: {e}"),
            ));
        }
    };

    host::info(&format!(
        "itch: get_profile successful for user_id={} username={}",
        me.user.id, me.user.username
    ));

    Ok(ExternalProfile {
        account_id: me.user.id.to_string(),
        display_name: me.user.display_name.or(Some(me.user.username)),
        profile_url: me.user.url,
        avatar_url: me.user.cover_url,
    })
}

fn http_post(url: &str, body: &str, content_type: &str) -> Result<String, String> {
    let response = host::http_request(
        &HttpRequest::new("POST", url)
            .header("Content-Type", content_type)
            .body_text(body),
    )
    .map_err(host_error)?;
    Ok(response.text().into_owned())
}

fn http_get_with_auth(url: &str, token: &str) -> Result<String, String> {
    let response = host::http_request(
        &HttpRequest::new("GET", url).header("Authorization", format!("Bearer {token}")),
    )
    .map_err(host_error)?;
    Ok(response.text().into_owned())
}

/// The message reported when a host call returns no response.
fn host_error(err: HostError) -> String {
    match err {
        HostError::Plugin(err) => err.message,
        HostError::NoResponse => "no response".to_string(),
        HostError::NotWasm => "host functions are unavailable outside wasm32".to_string(),
    }
}

fn urlencod(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                result.push(c);
            }
            _ => {
                for b in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", b));
                }
            }
        }
    }
    result
}
