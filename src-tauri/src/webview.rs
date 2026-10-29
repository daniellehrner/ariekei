// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! WebKitGTK runs a window's page in a process of its own. When that process ends (it
//! crashed, or went over WebKit's memory limit) the window shows only grey, while Ariekei and
//! its agents go on: the reason is logged and the page loaded again (its terminals reattach).
//! A page that ends again within a minute of that is left as it is, rather than loaded in a
//! loop.

#[cfg(target_os = "linux")]
use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

/// The windows whose page is watched.
#[cfg(target_os = "linux")]
static WATCHED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Watches the page of `webview` (once per window; called on every page load).
#[cfg(target_os = "linux")]
pub fn watch<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    let label = webview.label().to_owned();
    if !WATCHED.lock().unwrap().insert(label.clone()) {
        return;
    }
    let watched = webview.with_webview(move |platform| {
        use std::cell::Cell;
        use std::time::{Duration, Instant};
        use webkit2gtk::WebViewExt;
        let reloaded: Cell<Option<Instant>> = Cell::new(None);
        platform
            .inner()
            .connect_web_process_terminated(move |view, reason| {
                let again = reloaded
                    .get()
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(60));
                if again {
                    tracing::error!(
                        "the page of window {label} ended again ({reason:?}) within a minute of \
                         loading it again: left as it is; restart Ariekei"
                    );
                    return;
                }
                tracing::error!("the page of window {label} ended ({reason:?}): loading it again");
                reloaded.set(Some(Instant::now()));
                view.reload();
            });
    });
    if let Err(e) = watched {
        tracing::warn!("cannot watch the page of window {}: {e}", webview.label());
    }
}

#[cfg(not(target_os = "linux"))]
pub fn watch<R: tauri::Runtime>(_webview: &tauri::Webview<R>) {}

/// Window `label` is gone: a new one of that name is watched again.
pub fn forget(label: &str) {
    #[cfg(target_os = "linux")]
    WATCHED.lock().unwrap().remove(label);
    #[cfg(not(target_os = "linux"))]
    let _ = label;
}
