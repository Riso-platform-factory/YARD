//! The update check: asks the product's update address for the newest
//! release, and when it is newer than the product, installs it and shows a
//! toast, or shows a toast that it is available when it cannot be installed.

use futures::{AsyncReadExt as _, FutureExt as _};
use gpui::{App, AsyncApp};
use http_client::{AsyncBody, HttpClient, HttpRequestExt as _, RedirectPolicy, Request, Url};
use riso_product::Product;
use riso_shell::Extension;
use std::{cmp::Ordering, sync::Arc, time::Duration};
use tracing::Instrument as _;

/// The wait before the first check, so that the start is not slowed.
const FIRST_CHECK: Duration = Duration::from_secs(5);
/// The wait between two checks.
const NEXT_CHECK: Duration = Duration::from_secs(24 * 60 * 60);
/// The longest a check may take.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The longest answer read.
const MAX_ANSWER: usize = 64 * 1024;

/// The release file: its version, the address of its `.zip`, and the zip's
/// SHA-256 in hex.
#[derive(serde::Deserialize)]
struct Release {
    version: String,
    url: String,
    sha256: String,
}

/// What the checks of one run remember.
#[derive(Default)]
struct Seen {
    /// The versions whose "available" toast was shown.
    shown: Vec<semver::Version>,
    /// The versions installed.
    installed: Vec<semver::Version>,
    /// Whether `not installable` was logged.
    not_installable: bool,
}

/// Starts checking `url` for a version newer than `product`'s, installing it
/// and showing its toast through `extension`.
pub fn start(
    cx: &mut App,
    extension: &Extension,
    product: Product,
    url: &str,
    client: Arc<dyn HttpClient>,
) {
    let extension = extension.clone();
    let url = url.to_owned();
    cx.spawn(async move |cx| {
        let mut seen = Seen::default();
        cx.background_executor().timer(FIRST_CHECK).await;
        loop {
            let group = tracing::info_span!("update-check", current = product.version);
            check(cx, &extension, product, &url, &client, &mut seen)
                .instrument(group)
                .await;
            cx.background_executor().timer(NEXT_CHECK).await;
        }
    })
    .detach();
}

/// One check: the request, the comparison, and when newer, the install and
/// its toast, or the toast that it is available.
async fn check(
    cx: &mut AsyncApp,
    extension: &Extension,
    product: Product,
    url: &str,
    client: &Arc<dyn HttpClient>,
    seen: &mut Seen,
) {
    let fetch = cx
        .background_executor()
        .spawn(fetch(client.clone(), url.to_owned()).instrument(tracing::Span::current()));
    let timeout = cx.background_executor().timer(TIMEOUT);
    let release = futures::select_biased! {
        release = fetch.fuse() => release,
        _ = timeout.fuse() => Err(format!("no answer within {} s", TIMEOUT.as_secs())),
    };
    let compared = release.and_then(|release| {
        let current = semver::Version::parse(product.version).map_err(|error| {
            format!(
                "the product's version {:?} is unreadable: {error}",
                product.version
            )
        })?;
        let version = semver::Version::parse(&release.version)
            .map_err(|error| format!("the version {:?} is unreadable: {error}", release.version))?;
        Ok((
            version.cmp_precedence(&current) == Ordering::Greater,
            version,
            release,
        ))
    });
    let (version, release) = match compared {
        Ok((true, version, release)) => (version, release),
        Ok((false, version, _)) => {
            tracing::debug!(version = %version, "up to date");
            return;
        }
        Err(reason) => {
            let host = host(url);
            log::warn!(
                "update check of {host} failed: {}",
                without_addresses(&reason, &host)
            );
            return;
        }
    };
    tracing::debug!(version = %version, "newer");
    if seen
        .installed
        .iter()
        .any(|installed| installed.cmp_precedence(&version) == Ordering::Equal)
    {
        tracing::debug!(version = %version, "already installed");
        return;
    }
    match super::running_bundle() {
        Ok(bundle) => {
            if install(cx, product, &release, bundle, client).await {
                let text = format!("{} {version} is installed", product.name);
                let toast = riso_ui::Toast::new(riso_ui::ToastKind::Info, text).action(
                    "Restart",
                    |_, cx| {
                        tracing::debug!("restart");
                        cx.restart();
                    },
                );
                if show_toast(cx, extension, toast) {
                    tracing::debug!(version = %version, "toast shown");
                } else {
                    tracing::debug!(version = %version, "no window for the toast");
                }
                seen.installed.push(version);
            }
            return;
        }
        Err(reason) => {
            if !seen.not_installable {
                tracing::debug!("not installable {reason}");
                seen.not_installable = true;
            }
        }
    }
    if seen
        .shown
        .iter()
        .any(|shown| shown.cmp_precedence(&version) == Ordering::Equal)
    {
        tracing::debug!(version = %version, "already shown");
        return;
    }
    let text = format!("{} {version} is available", product.name);
    if show_toast(
        cx,
        extension,
        riso_ui::Toast::new(riso_ui::ToastKind::Info, text),
    ) {
        tracing::debug!(version = %version, "toast shown");
        seen.shown.push(version);
    } else {
        tracing::debug!(version = %version, "no window for the toast");
    }
}

/// Downloads and installs `release` over `bundle` on the background
/// executor, the download within its time limit; whether it was installed.
async fn install(
    cx: &mut AsyncApp,
    product: Product,
    release: &Release,
    bundle: std::path::PathBuf,
    client: &Arc<dyn HttpClient>,
) -> bool {
    let client = client.clone();
    let (version, url, sha256) = (
        release.version.clone(),
        release.url.clone(),
        release.sha256.clone(),
    );
    let limit = cx
        .background_executor()
        .timer(super::install::DOWNLOAD_TIMEOUT);
    cx.background_executor()
        .spawn(
            async move {
                let release = super::install::ReleaseFile {
                    name: product.name,
                    identifier: product.identifier,
                    version: &version,
                    url: &url,
                    sha256: &sha256,
                };
                super::install::install_within(client, bundle, release, limit).await
            }
            .instrument(tracing::Span::current()),
        )
        .await
        .is_ok()
}

/// Shows `toast` in the active window, or the first one; whether there was
/// a window for it.
fn show_toast(cx: &mut AsyncApp, extension: &Extension, toast: riso_ui::Toast) -> bool {
    cx.update(|cx| {
        let Some(window) = cx
            .active_window()
            .or_else(|| cx.windows().into_iter().next())
        else {
            return false;
        };
        window
            .update(cx, |_, window, cx| {
                extension.show_toast(toast, window, cx);
            })
            .is_ok()
    })
}

/// Asks `url` for the release file and reads it.
async fn fetch(client: Arc<dyn HttpClient>, url: String) -> Result<Release, String> {
    let request = Request::get(&url)
        .follow_redirects(RedirectPolicy::NoFollow)
        .body(AsyncBody::default())
        .map_err(|error| format!("invalid request: {error}"))?;
    let mut response = client
        .send(request)
        .await
        .map_err(|error| format!("request failed: {error:#}"))?;
    let status = response.status();
    if status.is_redirection() {
        return Err(format!(
            "the server answered {status}, a redirect, which is not followed"
        ));
    }
    if !status.is_success() {
        return Err(format!("the server answered {status}"));
    }
    let mut body = Vec::new();
    response
        .body_mut()
        .take(MAX_ANSWER as u64 + 1)
        .read_to_end(&mut body)
        .await
        .map_err(|error| format!("reading the answer failed: {error}"))?;
    if body.len() > MAX_ANSWER {
        return Err(format!(
            "the answer is longer than {} KiB",
            MAX_ANSWER / 1024
        ));
    }
    serde_json::from_slice(&body).map_err(|error| format!("invalid release file: {error}"))
}

/// The address's host, which the logs name instead of the address, whose path
/// or query can hold a token.
pub(super) fn host(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| String::from("an unreadable address"))
}

/// `reason` with every address in it, such as one an HTTP error quotes,
/// replaced by `host`.
pub(super) fn without_addresses(reason: &str, host: &str) -> String {
    let mut text = String::new();
    let mut rest = reason;
    while let Some(start) = ["http://", "https://"]
        .iter()
        .filter_map(|scheme| rest.find(scheme))
        .min()
    {
        text.push_str(&rest[..start]);
        text.push_str(host);
        let end = rest[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | '"' | '\'' | '>'))
            .map_or(rest.len(), |end| start + end);
        rest = &rest[end..];
    }
    text.push_str(rest);
    text
}
