//! `happyview-auth-steam`: Steam account linking over OpenID 2.0.
//!
//! Steam issues no OAuth token, so the SteamID64 lifted out of the verified
//! `openid.claimed_id` stands in for one; the Web API key supplies the
//! authority for the profile lookup.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::Deserialize;

use happyview_plugin_sdk::host::{self, HostError, HttpRequest};
use happyview_plugin_sdk::{
    auth_plugin, serde_json, AuthorizeUrlInput, CallbackInput, ExternalProfile, PluginError,
    PluginInfo, RefreshInput, TokenInput, TokenSet,
};

const STEAM_OPENID_URL: &str = "https://steamcommunity.com/openid/login";
const STEAM_API_BASE: &str = "https://api.steampowered.com";

auth_plugin! {
    info: PluginInfo::new("happyview-auth-steam", "Steam", "0.1.0")
        .icon_url("https://store.steampowered.com/favicon.ico")
        .auth_type("openid")
        .required_secrets(["PLUGIN_HAPPYVIEW_AUTH_STEAM_API_KEY"]),
    authorize_url: authorize_url,
    callback: callback,
    refresh: refresh,
    profile: profile,
}

#[derive(Deserialize)]
struct SteamPlayerSummary {
    response: SteamPlayersResponse,
}

#[derive(Deserialize)]
struct SteamPlayersResponse {
    players: Vec<SteamPlayer>,
}

#[derive(Deserialize)]
struct SteamPlayer {
    steamid: String,
    personaname: Option<String>,
    profileurl: Option<String>,
    avatarfull: Option<String>,
}

fn authorize_url(input: &AuthorizeUrlInput) -> Result<String, PluginError> {
    host::info("steam: get_authorize_url called");
    host::info(&format!(
        "steam: building OpenID URL with redirect_uri={}",
        input.redirect_uri
    ));

    // Steam uses claimed_id and identity as the same value for authentication.
    let return_to = format!("{}?state={}", input.redirect_uri, input.state);
    let params = [
        ("openid.ns", "http://specs.openid.net/auth/2.0"),
        ("openid.mode", "checkid_setup"),
        ("openid.return_to", return_to.as_str()),
        ("openid.realm", input.redirect_uri.as_str()),
        (
            "openid.identity",
            "http://specs.openid.net/auth/2.0/identifier_select",
        ),
        (
            "openid.claimed_id",
            "http://specs.openid.net/auth/2.0/identifier_select",
        ),
    ];

    let query: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencod(v)))
        .collect::<Vec<_>>()
        .join("&");

    host::info("steam: get_authorize_url completed successfully");
    Ok(format!("{}?{}", STEAM_OPENID_URL, query))
}

fn callback(input: &CallbackInput) -> Result<TokenSet, PluginError> {
    host::info("steam: handle_callback called");

    // Format: https://steamcommunity.com/openid/id/76561198012345678
    let steam_id = match input.param("openid.claimed_id") {
        Some(id) => match id.rfind('/') {
            Some(pos) => &id[pos + 1..],
            None => {
                host::error("steam: invalid claimed_id format");
                return Err(PluginError::new(
                    "INVALID_RESPONSE",
                    "Invalid claimed_id format",
                ));
            }
        },
        None => {
            host::error("steam: missing openid.claimed_id in callback");
            return Err(PluginError::new(
                "INVALID_RESPONSE",
                "Missing openid.claimed_id",
            ));
        }
    };

    host::info(&format!("steam: extracted steam_id={}", steam_id));

    // Verify the OpenID response with Steam: flip the mode to
    // check_authentication and POST every other openid.* param back.
    let mut verify_params: Vec<(&str, &str)> = Vec::new();
    verify_params.push(("openid.mode", "check_authentication"));
    for (key, value) in &input.params {
        if key.starts_with("openid.") && key != "openid.mode" {
            if let Some(v) = value.as_str() {
                verify_params.push((key.as_str(), v));
            }
        }
    }

    let verify_body: String = verify_params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencod(v)))
        .collect::<Vec<_>>()
        .join("&");

    host::info("steam: verifying OpenID response with Steam");
    match http_post(
        STEAM_OPENID_URL,
        &verify_body,
        "application/x-www-form-urlencoded",
    ) {
        Ok(response_body) => {
            // Steam answers with key-value pairs, one per line.
            if !response_body.contains("is_valid:true") {
                host::error("steam: OpenID verification failed - is_valid:true not found");
                return Err(PluginError::new(
                    "VERIFICATION_FAILED",
                    "Steam OpenID verification failed",
                ));
            }
            host::info("steam: OpenID verification successful");
        }
        Err(e) => {
            host::error(&format!("steam: OpenID verification request failed: {}", e));
            return Err(PluginError::new(
                "VERIFICATION_ERROR",
                format!("Failed to verify with Steam: {e}"),
            )
            .retryable());
        }
    }

    host::info(&format!(
        "steam: handle_callback completed successfully for steam_id={}",
        steam_id
    ));
    // Steam is OpenID 2.0, so there is no token to return. The SteamID is the
    // credential the API key is used against.
    Ok(TokenSet::new(steam_id, "SteamID"))
}

/// Steam IDs do not expire, so this hands the same one straight back.
fn refresh(input: &RefreshInput) -> Result<TokenSet, PluginError> {
    host::info("steam: refresh_tokens called (no-op for Steam)");
    Ok(TokenSet::new(input.refresh_token.clone(), "SteamID"))
}

fn profile(input: &TokenInput) -> Result<ExternalProfile, PluginError> {
    host::info("steam: get_profile called");

    let Some(api_key) = host::get_secret("API_KEY")? else {
        host::error("steam: API_KEY not configured");
        return Err(PluginError::new("MISSING_SECRET", "API_KEY not configured"));
    };

    let steam_id = &input.access_token;
    host::info(&format!(
        "steam: fetching profile for steam_id={}",
        steam_id
    ));

    let url = format!(
        "{}/ISteamUser/GetPlayerSummaries/v2/?key={}&steamids={}",
        STEAM_API_BASE, api_key, steam_id
    );

    let body = match http_get(&url) {
        Ok(b) => b,
        Err(e) => {
            host::error(&format!("steam: GetPlayerSummaries API error: {}", e));
            return Err(PluginError::new("HTTP_ERROR", e).retryable());
        }
    };

    let resp: SteamPlayerSummary = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            host::error(&format!("steam: failed to parse player summary: {}", e));
            return Err(PluginError::new(
                "INVALID_RESPONSE",
                format!("Parse error: {e}"),
            ));
        }
    };

    let Some(player) = resp.response.players.first() else {
        host::error(&format!(
            "steam: player not found for steam_id={}",
            steam_id
        ));
        return Err(PluginError::new("NOT_FOUND", "Player not found"));
    };

    host::info(&format!(
        "steam: get_profile completed for {} ({})",
        player.personaname.as_deref().unwrap_or("unknown"),
        steam_id
    ));

    Ok(ExternalProfile {
        account_id: player.steamid.clone(),
        display_name: player.personaname.clone(),
        profile_url: player.profileurl.clone(),
        avatar_url: player.avatarfull.clone(),
    })
}

fn http_get(url: &str) -> Result<String, String> {
    let response = host::http_request(&HttpRequest::new("GET", url)).map_err(host_error)?;
    Ok(response.text().into_owned())
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
