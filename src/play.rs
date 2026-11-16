//! Joining a game the way the site's `launchGame` (`api-games.js`) does:
//!
//! 1. `GET /game/get-join-script?placeId=…[&jobId=…]` returns plain text with a
//!    `ticket=` in it and a `&<year>=true` flag for the client version.
//! 2. That becomes `hexium://join?place=…&ticket=…&year=…[&jobId=…]`, opened
//!    through Windows so the installed Hexium launcher starts.
//! 3. The same URL is POSTed to the launcher's local bridge on
//!    `127.0.0.1:17341/join`, retried for up to 20 seconds while it starts up.
//!
//! (A JSON answer with `clientArgs` becomes `rbxeconsim:<clientArgs>` instead,
//! as on the site.) The URL carries a sign-in ticket: it is never logged.

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::api::{ApiClient, ApiError};

const YEARS: [&str; 6] = ["2015", "2016", "2017", "2018", "2020", "2021"];
const DEFAULT_YEAR: &str = "2016";
const BRIDGE: &str = "http://127.0.0.1:17341/join";

/// The launch URL for a join script's body.
pub fn launch_url(body: &str, place_id: u64, job_id: Option<&str>) -> Result<String, String> {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(arguments) = value.get("clientArgs").and_then(Value::as_str) {
            return Ok(format!("rbxeconsim:{arguments}"));
        }
        return Err("the site answered in a format the client does not know".to_string());
    }
    let ticket = body
        .split("ticket=")
        .nth(1)
        .map(|rest| rest.split(['&', '"', '\'']).next().unwrap_or_default())
        .filter(|ticket| !ticket.is_empty())
        .ok_or("the site gave no join ticket (is the session still signed in?)")?;
    let year = YEARS
        .iter()
        .find(|year| body.contains(&format!("&{year}=true")))
        .copied()
        .unwrap_or(DEFAULT_YEAR);
    let mut url = format!(
        "hexium://join?place={place_id}&ticket={}&year={year}",
        crate::api::hub::encode(ticket)
    );
    if let Some(job) = job_id {
        url.push_str(&format!("&jobId={}", crate::api::hub::encode(job)));
    }
    Ok(url)
}

/// Fetches a ticket and starts the game. Returns the client year it asked for.
pub async fn launch(api: &ApiClient, place_id: u64, job_id: Option<&str>) -> Result<String, String> {
    let mut query = vec![("placeId", place_id.to_string())];
    if let Some(job) = job_id {
        query.push(("jobId", job.to_string()));
    }
    let body = api
        .get_text("/game/get-join-script", &query)
        .await
        .map_err(|error: ApiError| error.to_string())?;
    let url = launch_url(&body, place_id, job_id)?;
    let year = url
        .split("&year=")
        .nth(1)
        .map(|rest| rest.split('&').next().unwrap_or_default().to_string())
        .unwrap_or_default();

    open_url(&url)?;
    // Best effort, as on the site: the launcher may not run a bridge at all.
    let _ = tell_bridge(&url).await;
    Ok(year)
}

async fn tell_bridge(url: &str) -> Result<(), ()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|_| ())?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let sent = client
            .post(BRIDGE)
            .header("content-type", "text/plain")
            .body(url.to_string())
            .send()
            .await;
        if sent.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    Err(())
}

/// The program Windows starts for `hexium://` (its exe name), if any.
#[cfg(windows)]
pub fn launcher_registered() -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CLASSES_ROOT, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
    let mut buffer = vec![0u16; 1024];
    let mut bytes = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            &HSTRING::from(r"hexium\shell\open\command"),
            &HSTRING::new(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let command = String::from_utf16_lossy(&buffer[..(bytes as usize / 2).saturating_sub(1)]);
    // Just the exe's file name; the full command line is not needed here.
    let exe = command.trim().trim_start_matches('"').split(".exe").next().unwrap_or_default();
    let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    Some(format!("{name}.exe"))
}

#[cfg(not(windows))]
pub fn launcher_registered() -> Option<String> {
    None
}

/// Hands the URL to Windows, which starts whatever is registered for it.
#[cfg(windows)]
fn open_url(url: &str) -> Result<(), String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let result = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // ShellExecute reports success as a value above 32.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("Windows could not start the Hexium launcher. Is it installed? (Playing once from hexium.wtf installs it.)".to_string())
    }
}

#[cfg(not(windows))]
fn open_url(_url: &str) -> Result<(), String> {
    Err("launching games is only supported on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_hexium_url_like_the_site() {
        let body = r#"-a "https://example/Login/Negotiate.ashx" -t "1" -j "https://example/join?ticket=AB+C/1&x=1&2021=true""#;
        assert_eq!(
            launch_url(body, 42, None).unwrap(),
            "hexium://join?place=42&ticket=AB%2BC%2F1&year=2021"
        );
        assert_eq!(
            launch_url("ticket=T", 7, Some("job 1")).unwrap(),
            "hexium://join?place=7&ticket=T&year=2016&jobId=job%201"
        );
        assert!(launch_url("no ticket here", 1, None).is_err());
        assert_eq!(launch_url(r#"{"clientArgs":"--x 1"}"#, 1, None).unwrap(), "rbxeconsim:--x 1");
    }
}
