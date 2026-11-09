//! Sign-in: Apple's own icloud.com page in a locked-down web view.
//! The user types the password into Apple's page; we only read the resulting session cookies.

use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::api::{RawCookie, USER_AGENT};
use crate::AppState;

const LOGIN_LABEL: &str = "login";
const START: &str = "https://www.icloud.com/iclouddrive/";

/// Only Apple hosts may load in the sign-in window.
fn allowed(u: &Url) -> bool {
    if u.scheme() == "about" {
        return true;
    }
    if u.scheme() != "https" {
        return false;
    }
    let h = u.host_str().unwrap_or_default();
    ["icloud.com", "apple.com", "cdn-apple.com", "apple-cloudkit.com", "icloud-content.com", "mzstatic.com", "apple-mapkit.com"]
        .iter()
        .any(|d| h == *d || h.ends_with(&format!(".{d}")))
}

pub fn read_cookies<R: tauri::Runtime>(w: &tauri::WebviewWindow<R>) -> Vec<RawCookie> {
    w.cookies()
        .unwrap_or_default()
        .into_iter()
        .map(|c| RawCookie {
            name: c.name().to_string(),
            value: c.value().to_string(),
            domain: c.domain().unwrap_or("icloud.com").to_string(),
            path: c.path().unwrap_or("/").to_string(),
        })
        .collect()
}

fn has_session(cookies: &[RawCookie]) -> bool {
    cookies.iter().any(|c| c.name.starts_with("X-APPLE-WEBAUTH-TOKEN") || c.name == "X-APPLE-WEBAUTH-USER")
}

/// Silent sign-in at launch from cookies kept by the system web view store.
pub async fn resume(app: &AppHandle) -> bool {
    let Some(main) = app.get_webview_window("main") else { return false };
    let cookies = read_cookies(&main);
    if !has_session(&cookies) {
        return false;
    }
    let state = app.state::<AppState>();
    state.icloud.load_cookies(&cookies);
    state.icloud.validate().await.is_ok()
}

pub fn open(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
        w.set_focus()?;
        return Ok(());
    }
    let w = WebviewWindowBuilder::new(app, LOGIN_LABEL, WebviewUrl::External(START.parse().expect("url")))
        .title("Sign in to iCloud")
        .inner_size(1000.0, 780.0)
        .min_inner_size(720.0, 600.0)
        .center()
        .user_agent(USER_AGENT)
        .on_navigation(allowed)
        .build()?;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(1200));
        loop {
            tick.tick().await;
            let Some(win) = app.get_webview_window(LOGIN_LABEL) else { break };
            let cookies = read_cookies(&win);
            if !has_session(&cookies) {
                continue;
            }
            // Let the Drive page finish loading so Apple can add its PCS cookies (Advanced Data Protection).
            tokio::time::sleep(Duration::from_millis(2500)).await;
            let cookies = read_cookies(&win);
            let state = app.state::<AppState>();
            state.icloud.load_cookies(&cookies);
            match state.icloud.validate().await {
                Ok(account) => {
                    tracing::info!(target: "auth", "signed in");
                    let _ = app.emit("auth://changed", Some(account));
                    let _ = win.close();
                    break;
                }
                Err(e) => tracing::warn!(target: "auth", error = %e, "validate failed, still waiting"),
            }
        }
    });
    let _ = w;
    Ok(())
}
