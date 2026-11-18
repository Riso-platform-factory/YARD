//! The update check: asks the product's update address for the newest
//! release, and shows a toast when it is newer than the product.

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

/// The release file. This feature reads `version`; `url` and `sha256` must
/// be present for the release to be installable.
#[derive(serde::Deserialize)]
struct Release {
    version: String,
    #[allow(dead_code)]
    url: String,
    #[allow(dead_code)]
    sha256: String,
}

/// Starts checking `url` for a version newer than `product`'s, showing its
/// toast through `extension`.
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
        // The versions whose toast was shown in this run.
        let mut shown = Vec::new();
        cx.background_executor().timer(FIRST_CHECK).await;
        loop {
            let group = tracing::info_span!("update-check", current = product.version);
            check(cx, &extension, product, &url, &client, &mut shown)
                .instrument(group)
                .await;
            cx.background_executor().timer(NEXT_CHECK).await;
        }
    })
    .detach();
}

/// One check: the request, the comparison, and the toast when newer.
async fn check(
    cx: &mut AsyncApp,
    extension: &Extension,
    product: Product,
    url: &str,
    client: &Arc<dyn HttpClient>,
    shown: &mut Vec<semver::Version>,
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
        ))
    });
    let version = match compared {
        Ok((true, version)) => version,
        Ok((false, version)) => {
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
    if shown
        .iter()
        .any(|shown| shown.cmp_precedence(&version) == Ordering::Equal)
    {
        tracing::debug!(version = %version, "already shown");
        return;
    }
    let text = format!("{} {version} is available", product.name);
    let toasted = cx.update(|cx| {
        let Some(window) = cx
            .active_window()
            .or_else(|| cx.windows().into_iter().next())
        else {
            return false;
        };
        window
            .update(cx, |_, window, cx| {
                extension.show_toast(
                    riso_ui::Toast::new(riso_ui::ToastKind::Info, text),
                    window,
                    cx,
                );
            })
            .is_ok()
    });
    if toasted {
        tracing::debug!(version = %version, "toast shown");
        shown.push(version);
    } else {
        tracing::debug!(version = %version, "no window for the toast");
    }
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
fn host(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| String::from("an unreadable address"))
}

/// `reason` with every address in it, such as one an HTTP error quotes,
/// replaced by `host`.
fn without_addresses(reason: &str, host: &str) -> String {
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
