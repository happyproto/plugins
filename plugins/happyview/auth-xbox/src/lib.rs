//! `happyview-auth-xbox`: Xbox account linking.
//!
//! Microsoft OAuth 2.0 gets an RPS ticket; the profile read then walks the two
//! further exchanges Xbox Live requires — user authenticate, then XSTS
//! authorize — to reach a gamertag and XUID.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use happyview_plugin_sdk::host::{self, HostError, HttpRequest};
use happyview_plugin_sdk::{
    auth_plugin, serde_json, AuthorizeUrlInput, CallbackInput, ExternalProfile, PluginError,
    PluginInfo, RefreshInput, TokenInput, TokenSet,
};

const MS_AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const MS_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
/// Fallback when the host supplies no `redirect_uri`.
const DEFAULT_REDIRECT_URI: &str = "http://localhost:3001/dashboard/settings/accounts/";

auth_plugin! {
    info: PluginInfo::new("happyview-auth-xbox", "Xbox", "0.1.0")
        .auth_type("oauth2")
        .required_secrets([
            "PLUGIN_HAPPYVIEW_AUTH_XBOX_CLIENT_ID",
            "PLUGIN_HAPPYVIEW_AUTH_XBOX_CLIENT_SECRET",
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

#[derive(Serialize)]
struct XblAuthRequest {
    #[serde(rename = "RelyingParty")]
    relying_party: String,
    #[serde(rename = "TokenType")]
    token_type: String,
    #[serde(rename = "Properties")]
    properties: XblAuthProperties,
}

#[derive(Serialize)]
struct XblAuthProperties {
    #[serde(rename = "AuthMethod")]
    auth_method: String,
    #[serde(rename = "SiteName")]
    site_name: String,
    #[serde(rename = "RpsTicket")]
    rps_ticket: String,
}

#[derive(Deserialize)]
struct XblAuthResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XblDisplayClaims,
}

#[derive(Deserialize)]
struct XblDisplayClaims {
    xui: Vec<XblUserInfo>,
}

#[derive(Deserialize)]
struct XblUserInfo {
    uhs: String,
    #[allow(dead_code)]
    xid: Option<String>,
    #[allow(dead_code)]
    gtg: Option<String>,
}

#[derive(Serialize)]
struct XstsAuthRequest {
    #[serde(rename = "RelyingParty")]
    relying_party: String,
    #[serde(rename = "TokenType")]
    token_type: String,
    #[serde(rename = "Properties")]
    properties: XstsAuthProperties,
}

#[derive(Serialize)]
struct XstsAuthProperties {
    #[serde(rename = "SandboxId")]
    sandbox_id: String,
    #[serde(rename = "UserTokens")]
    user_tokens: Vec<String>,
}

#[derive(Deserialize)]
struct XstsAuthResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XstsDisplayClaims,
}

#[derive(Deserialize)]
struct XstsDisplayClaims {
    xui: Vec<XstsUserInfo>,
}

#[derive(Deserialize)]
struct XstsUserInfo {
    #[allow(dead_code)]
    uhs: String,
    xid: String,
    gtg: String,
}

#[derive(Deserialize)]
struct XboxProfileResponse {
    #[serde(rename = "profileUsers")]
    profile_users: Vec<XboxProfileUser>,
}

#[derive(Deserialize)]
struct XboxProfileUser {
    settings: Vec<XboxProfileSetting>,
}

#[derive(Deserialize)]
struct XboxProfileSetting {
    id: String,
    value: String,
}

fn authorize_url(input: &AuthorizeUrlInput) -> Result<String, PluginError> {
    host::info("xbox: get_authorize_url called");
    host::info(&format!(
        "xbox: get_authorize_url redirect_uri={}",
        input.redirect_uri
    ));

    let Some(client_id) = host::get_secret("CLIENT_ID")? else {
        host::error("xbox: CLIENT_ID not configured");
        return Err(PluginError::new("CONFIG_ERROR", "CLIENT_ID not configured"));
    };

    let scopes = "XboxLive.signin XboxLive.offline_access";
    let url = format!(
        "{}?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}",
        MS_AUTHORIZE_URL,
        urlencoding_encode(&client_id),
        urlencoding_encode(&input.redirect_uri),
        urlencoding_encode(scopes),
        urlencoding_encode(&input.state)
    );

    host::info("xbox: get_authorize_url returning URL");
    Ok(url)
}

fn callback(input: &CallbackInput) -> Result<TokenSet, PluginError> {
    host::info("xbox: handle_callback called");

    let Some(code) = input.param("code") else {
        host::error("xbox: handle_callback no authorization code received");
        return Err(PluginError::new(
            "AUTH_FAILED",
            "No authorization code received",
        ));
    };

    host::info("xbox: handle_callback received authorization code");

    let Some(client_id) = host::get_secret("CLIENT_ID")? else {
        host::error("xbox: CLIENT_ID not configured");
        return Err(PluginError::new("CONFIG_ERROR", "CLIENT_ID not configured"));
    };

    let Some(client_secret) = host::get_secret("CLIENT_SECRET")? else {
        host::error("xbox: CLIENT_SECRET not configured");
        return Err(PluginError::new(
            "CONFIG_ERROR",
            "CLIENT_SECRET not configured",
        ));
    };

    // The host passes the redirect_uri through as a callback parameter.
    let redirect_uri = input.param("redirect_uri").unwrap_or(DEFAULT_REDIRECT_URI);

    host::info(&format!(
        "xbox: handle_callback redirect_uri={}",
        redirect_uri
    ));

    host::info("xbox: exchanging code for Microsoft token");
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
            host::error(&format!("xbox: MS token exchange failed: {}", e));
            return Err(
                PluginError::new("TOKEN_ERROR", format!("Failed to get MS token: {e}")).retryable(),
            );
        }
    };

    let ms_token: MsTokenResponse = match serde_json::from_str(&ms_token_resp) {
        Ok(t) => t,
        Err(e) => {
            host::error(&format!("xbox: failed to parse MS token response: {}", e));
            return Err(PluginError::new(
                "TOKEN_ERROR",
                format!("Failed to parse MS token: {e}"),
            ));
        }
    };

    host::info("xbox: handle_callback MS token exchange successful");
    // The MS access token is the stored access_token; `profile` exchanges it
    // for Xbox Live tokens when it needs them.
    Ok(token_set(ms_token))
}

fn refresh(input: &RefreshInput) -> Result<TokenSet, PluginError> {
    host::info("xbox: refresh_tokens called");

    let Some(client_id) = host::get_secret("CLIENT_ID")? else {
        host::error("xbox: CLIENT_ID not configured");
        return Err(PluginError::new("CONFIG_ERROR", "CLIENT_ID not configured"));
    };

    let Some(client_secret) = host::get_secret("CLIENT_SECRET")? else {
        host::error("xbox: CLIENT_SECRET not configured");
        return Err(PluginError::new(
            "CONFIG_ERROR",
            "CLIENT_SECRET not configured",
        ));
    };

    host::info("xbox: refreshing MS token");
    let body = format!(
        "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
        urlencoding_encode(&client_id),
        urlencoding_encode(&client_secret),
        urlencoding_encode(&input.refresh_token)
    );

    let resp = match http_post(MS_TOKEN_URL, &body, "application/x-www-form-urlencoded") {
        Ok(r) => r,
        Err(e) => {
            host::error(&format!("xbox: token refresh failed: {}", e));
            return Err(
                PluginError::new("TOKEN_ERROR", format!("Failed to refresh: {e}")).retryable(),
            );
        }
    };

    let ms_token: MsTokenResponse = match serde_json::from_str(&resp) {
        Ok(t) => t,
        Err(e) => {
            host::error(&format!("xbox: failed to parse refresh response: {}", e));
            return Err(PluginError::new(
                "TOKEN_ERROR",
                format!("Failed to parse: {e}"),
            ));
        }
    };

    host::info("xbox: refresh_tokens successful");
    Ok(token_set(ms_token))
}

fn profile(input: &TokenInput) -> Result<ExternalProfile, PluginError> {
    host::info("xbox: get_profile called");

    host::info("xbox: exchanging MS token for XBL token");
    let (xbl_token, user_hash) = match get_xbl_token(&input.access_token) {
        Ok(t) => t,
        Err(e) => {
            host::error(&format!("xbox: XBL token exchange failed: {}", e));
            return Err(PluginError::new("AUTH_ERROR", e).retryable());
        }
    };

    host::info("xbox: exchanging XBL token for XSTS token");
    let (xsts_token, xuid, gamertag) = match get_xsts_token(&xbl_token) {
        Ok(t) => t,
        Err(e) => {
            host::error(&format!("xbox: XSTS token exchange failed: {}", e));
            return Err(PluginError::new("AUTH_ERROR", e).retryable());
        }
    };

    host::info(&format!(
        "xbox: got XSTS token for xuid={} gamertag={}",
        xuid, gamertag
    ));

    host::info("xbox: fetching profile details");
    let auth_header = format!("XBL3.0 x={};{}", user_hash, xsts_token);
    let profile_url = format!(
        "https://profile.xboxlive.com/users/xuid({})/profile/settings?settings=Gamertag,GameDisplayPicRaw",
        xuid
    );

    // The gamertag from the XSTS claims is already good enough, so a failure
    // here is logged and fallen back on rather than surfaced.
    let (display_name, avatar_url) = match http_get_with_auth(&profile_url, &auth_header) {
        Ok(resp) => {
            if let Ok(profile_resp) = serde_json::from_str::<XboxProfileResponse>(&resp) {
                let mut name = gamertag.clone();
                let mut avatar = None;
                if let Some(user) = profile_resp.profile_users.first() {
                    for setting in &user.settings {
                        if setting.id == "Gamertag" {
                            name = setting.value.clone();
                        } else if setting.id == "GameDisplayPicRaw" {
                            avatar = Some(setting.value.clone());
                        }
                    }
                }
                host::info(&format!("xbox: profile fetched for {}", name));
                (name, avatar)
            } else {
                host::info("xbox: using gamertag from XSTS (profile parse failed)");
                (gamertag.clone(), None)
            }
        }
        Err(e) => {
            host::info(&format!(
                "xbox: profile fetch failed ({}), using gamertag from XSTS",
                e
            ));
            (gamertag.clone(), None)
        }
    };

    host::info("xbox: get_profile completed successfully");
    Ok(ExternalProfile {
        account_id: xuid,
        display_name: Some(display_name.clone()),
        profile_url: Some(format!(
            "https://www.xbox.com/en-US/play/user/{}",
            display_name
        )),
        avatar_url,
    })
}

fn get_xbl_token(ms_access_token: &str) -> Result<(String, String), String> {
    host::info("xbox: get_xbl_token - authenticating with Xbox Live");

    let req = XblAuthRequest {
        relying_party: "http://auth.xboxlive.com".into(),
        token_type: "JWT".into(),
        properties: XblAuthProperties {
            auth_method: "RPS".into(),
            site_name: "user.auth.xboxlive.com".into(),
            rps_ticket: format!("d={ms_access_token}"),
        },
    };

    let body = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    let resp = http_post(XBL_AUTH_URL, &body, "application/json").map_err(|e| {
        host::error(&format!("xbox: XBL auth request failed: {}", e));
        e
    })?;

    let xbl_resp: XblAuthResponse = serde_json::from_str(&resp).map_err(|e| {
        host::error(&format!("xbox: failed to parse XBL response: {}", e));
        format!("Failed to parse XBL response: {e}")
    })?;

    let user_hash = xbl_resp
        .display_claims
        .xui
        .first()
        .map(|u| u.uhs.clone())
        .ok_or_else(|| {
            host::error("xbox: no user hash in XBL response");
            "No user hash in XBL response".to_string()
        })?;

    host::info("xbox: get_xbl_token successful");
    Ok((xbl_resp.token, user_hash))
}

fn get_xsts_token(xbl_token: &str) -> Result<(String, String, String), String> {
    host::info("xbox: get_xsts_token - getting XSTS token");

    let req = XstsAuthRequest {
        relying_party: "http://xboxlive.com".into(),
        token_type: "JWT".into(),
        properties: XstsAuthProperties {
            sandbox_id: "RETAIL".into(),
            user_tokens: vec![xbl_token.to_string()],
        },
    };

    let body = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    let resp = http_post(XSTS_AUTH_URL, &body, "application/json").map_err(|e| {
        host::error(&format!("xbox: XSTS auth request failed: {}", e));
        e
    })?;

    let xsts_resp: XstsAuthResponse = serde_json::from_str(&resp).map_err(|e| {
        host::error(&format!("xbox: failed to parse XSTS response: {}", e));
        format!("Failed to parse XSTS response: {e}")
    })?;

    let user_info = xsts_resp.display_claims.xui.first().ok_or_else(|| {
        host::error("xbox: no user info in XSTS response");
        "No user info in XSTS response".to_string()
    })?;

    host::info(&format!(
        "xbox: get_xsts_token successful for xid={}",
        user_info.xid
    ));
    Ok((
        xsts_resp.token,
        user_info.xid.clone(),
        user_info.gtg.clone(),
    ))
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

/// Xbox Live wants the `XBL3.0` header verbatim, not a `Bearer` prefix.
fn http_get_with_auth(url: &str, token: &str) -> Result<String, String> {
    let response = host::http_request(
        &HttpRequest::new("GET", url)
            .header("Authorization", token)
            .header("x-xbl-contract-version", "2"),
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
