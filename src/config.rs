//! Local settings. The session cookie is the only sensitive value and it lives
//! here, outside the repository, in the OS config directory.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_ORIGIN: &str = "https://hexium.wtf";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_origin")]
    pub origin: String,
    #[serde(default)]
    pub session_cookie: Option<String>,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Accent colour key, one of `ui::ACCENTS`.
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_width")]
    pub window_width: f32,
    #[serde(default = "default_height")]
    pub window_height: f32,
    /// Tell the other person in a chat when you are typing, as the site does.
    #[serde(default = "default_true")]
    pub share_typing: bool,
}

fn default_true() -> bool {
    true
}

fn default_width() -> f32 {
    1300.0
}

fn default_height() -> f32 {
    850.0
}

fn default_origin() -> String {
    DEFAULT_ORIGIN.to_string()
}

fn default_theme() -> String {
    "midnight".to_string()
}

fn default_accent() -> String {
    "coral".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            origin: default_origin(),
            session_cookie: None,
            theme: default_theme(),
            accent: default_accent(),
            window_width: default_width(),
            window_height: default_height(),
            share_typing: true,
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("hexium-client");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("hexium-client");
    }
    PathBuf::from(".hexium-client")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn load() -> Config {
    let mut config = std::fs::read_to_string(config_path())
        .ok()
        .and_then(|raw| serde_json::from_str::<Config>(&raw).ok())
        .unwrap_or_default();

    if config.origin.trim().is_empty() {
        config.origin = default_origin();
    }
    if config.session_cookie.is_none() {
        if let Ok(from_env) = std::env::var("HEXIUM_COOKIE") {
            let cookie = Config::normalize_cookie(&from_env);
            if !cookie.is_empty() {
                config.session_cookie = Some(cookie);
            }
        }
    }
    config
}

impl Config {
    pub fn save(&self) {
        let directory = config_dir();
        let _ = std::fs::create_dir_all(&directory);
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(config_path(), raw);
        }
    }

    /// Accepts the raw value, a `name=value` pair, or a pasted Set-Cookie line.
    pub fn normalize_cookie(input: &str) -> String {
        let mut value = input.trim().trim_matches('"').trim();
        if let Some(rest) = value.strip_prefix(".ROBLOSECURITY=") {
            value = rest;
        }
        if let Some(rest) = value.strip_prefix("ROBLOSECURITY=") {
            value = rest;
        }
        if let Some((head, _)) = value.split_once(';') {
            value = head;
        }
        value.trim().to_string()
    }

    pub fn masked_session(&self) -> Option<String> {
        self.session_cookie.as_ref().map(|cookie| {
            let tail: String = cookie.chars().rev().take(4).collect();
            let tail: String = tail.chars().rev().collect();
            format!("stored · {} chars · …{tail}", cookie.chars().count())
        })
    }
}
