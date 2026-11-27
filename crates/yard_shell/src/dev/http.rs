use crate::Extension;
use crate::shell::http_client_slot;
use gpui::{App, Global};
use std::sync::atomic::{AtomicBool, Ordering};
use yard_http::HttpClient;

struct AppHttpClient(HttpClient);

impl Global for AppHttpClient {}

static REQUESTED_WITHOUT_CLIENT: AtomicBool = AtomicBool::new(false);

/// Gives extensions access to the application's shared HTTP client.
pub fn set_http_client(cx: &mut App, client: HttpClient) {
    let _group = tracing::info_span!("http-client-set").entered();
    if cx.try_global::<AppHttpClient>().is_some() {
        log::error!("the application HTTP client must be set once");
        return;
    }
    if let Some(waiting) =
        http_client_slot(cx).and_then(|slot| slot.get()?.downcast_ref::<HttpClient>())
    {
        waiting.activate(&client);
    }
    cx.set_global(AppHttpClient(client));
    tracing::debug!("shared client installed");
}

impl Extension {
    pub fn http_client(&self, cx: &App) -> HttpClient {
        if let Some(client) = cx.try_global::<AppHttpClient>() {
            return client.0.for_caller(&self.name);
        }
        if !REQUESTED_WITHOUT_CLIENT.swap(true, Ordering::Relaxed) {
            log::error!(
                "extension HTTP is unavailable because the application did not set a client"
            );
        }
        let Some(slot) = http_client_slot(cx) else {
            return HttpClient::unavailable().for_caller(&self.name);
        };
        slot.get_or_init(|| Box::new(HttpClient::unavailable()))
            .downcast_ref::<HttpClient>()
            .expect("the HTTP client slot stores an HTTP client")
            .for_caller(&self.name)
    }
}
