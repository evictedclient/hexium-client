//! Updates from GitHub releases. Every push to `main` builds a release
//! tagged `v<version>`; on start (and from Settings) the client reads the
//! latest release, downloads its exe next to the running one and swaps the
//! two (Windows lets a running exe be renamed). The new version runs from
//! the next start, or straight away with "Restart now".

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use eframe::egui;
use serde::Deserialize;

/// Where releases come from.
pub const REPO: &str = "evictedclient/hexium-client";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Downloading(String),
    /// Installed; runs after a restart.
    Ready(String),
    Failed(String),
}

fn status_cell() -> &'static Mutex<Status> {
    static STATUS: Mutex<Status> = Mutex::new(Status::Idle);
    &STATUS
}

pub fn status() -> Status {
    status_cell().lock().unwrap().clone()
}

fn set(status: Status, egui: &egui::Context) {
    *status_cell().lock().unwrap() = status;
    egui.request_repaint();
}

/// Whether this build updates itself: release builds only, so a dev build
/// from `cargo run` is never replaced.
pub fn enabled() -> bool {
    !cfg!(debug_assertions)
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// "v0.2.15" or "0.2.15" as numbers.
pub fn parse_version(text: &str) -> Vec<u64> {
    text.trim().trim_start_matches('v').split(['.', '-', '+']).map_while(|part| part.parse().ok()).collect()
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    let (latest, current) = (parse_version(latest), parse_version(current));
    !latest.is_empty() && latest > current
}

fn old_path(exe: &std::path::Path) -> PathBuf {
    exe.with_extension("exe.old")
}

/// Removes the exe an earlier update left behind.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
        let _ = std::fs::remove_file(exe.with_extension("exe.new"));
    }
}

/// Checks now and then every six hours while the client runs.
pub fn start_background(rt: &tokio::runtime::Handle, egui: egui::Context) {
    let handle = rt.clone();
    rt.spawn(async move {
        loop {
            check(&handle, egui.clone());
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

/// Checks for a newer release and, if there is one, installs it. Runs in the
/// background; progress shows through [`status`].
pub fn check(rt: &tokio::runtime::Handle, egui: egui::Context) {
    if matches!(status(), Status::Checking | Status::Downloading(_) | Status::Ready(_)) {
        return;
    }
    set(Status::Checking, &egui);
    rt.spawn(async move {
        let result = run(Some(&egui), false).await;
        match result {
            Ok(Some(version)) => set(Status::Ready(version), &egui),
            Ok(None) => set(Status::UpToDate, &egui),
            Err(error) => set(Status::Failed(error), &egui),
        }
    });
}

/// On launch, before the window opens: installs a newer release if GitHub
/// answers quickly. True when one was installed (the caller restarts).
pub fn update_on_launch() -> bool {
    if !enabled() {
        return false;
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
        return false;
    };
    matches!(runtime.block_on(run(None, true)), Ok(Some(_)))
}

/// Finds and installs a newer release. `quick`: give up on GitHub after a
/// few seconds (launch must not hang offline).
async fn run(egui: Option<&egui::Context>, quick: bool) -> Result<Option<String>, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("hexium-client-updater/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(if quick { 4 } else { 20 }))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(if quick { 5 } else { 30 }))
        .send()
        .await
        .map_err(|error| format!("could not reach GitHub: {error}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("no releases found (the repository may be private)".to_string());
    }
    if !response.status().is_success() {
        return Err(format!("GitHub answered {}", response.status()));
    }
    let release: Release = response.json().await.map_err(|error| format!("unreadable release: {error}"))?;
    if release.draft || release.prerelease || !is_newer(&release.tag_name, CURRENT) {
        return Ok(None);
    }
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with(".exe"))
        .ok_or_else(|| format!("{} has no exe attached", release.tag_name))?;
    if let Some(egui) = egui {
        set(Status::Downloading(release.tag_name.clone()), egui);
    }
    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("download failed: {error}"))?
        .bytes()
        .await
        .map_err(|error| format!("download failed: {error}"))?;
    // A Windows program, and not an error page.
    if bytes.len() < 1_000_000 || !bytes.starts_with(b"MZ") {
        return Err("the download is not a Windows program".to_string());
    }
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    // The same exe under a newer tag (a release whose version was not set):
    // installing it would only repeat on every check.
    if std::fs::read(&exe).is_ok_and(|current| current == bytes.as_ref()) {
        return Ok(None);
    }
    let new = exe.with_extension("exe.new");
    std::fs::write(&new, &bytes).map_err(|error| format!("could not save the update: {error}"))?;
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|error| format!("could not replace the exe: {error}"))?;
    if let Err(error) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("could not replace the exe: {error}"));
    }
    Ok(Some(release.tag_name))
}

/// Starts the (updated) exe and leaves. The new process waits for this one
/// to let go of the one-client lock.
pub fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        if std::process::Command::new(exe).arg("--updated").spawn().is_ok() {
            std::process::exit(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(is_newer("v0.2.10", "0.2.9"));
        assert!(is_newer("v1.0.0", "0.9.99"));
        assert!(!is_newer("v0.2.9", "0.2.9"));
        assert!(!is_newer("v0.2.8", "0.2.9"));
        assert!(!is_newer("nightly", "0.2.9"));
        assert_eq!(parse_version("v0.3.4"), vec![0, 3, 4]);
    }
}
