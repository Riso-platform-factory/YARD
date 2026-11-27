# HTTP tour

`cargo run dev` loads the review extension. Set `HTTP_TOUR` to the URL you want to fetch. The card appears only when that variable is set. Click **Fetch** to see the status, bytes received (up to 1 MiB), and elapsed time, or an error.

Use a fresh settings directory for the tour.

```sh
export YARD_HOME="$(mktemp -d)"
mkdir -p "$YARD_HOME/config"
```

Run each application command from the repository root. The request trace shows `http-request` with caller `http_tour`, method, and host. A completed request records its status and elapsed time.

## Public HTTPS and a refused redirect

| Command | What to look for |
| --- | --- |
| `HTTP_TOUR=https://example.com/ cargo run dev` | **Fetch** reports a successful HTTPS response and a byte count. |
| `HTTP_TOUR='https://httpbin.org/redirect-to?url=http%3A%2F%2Fexample.com%2F' cargo run dev` | **Fetch** reports that the HTTPS-to-HTTP redirect was refused. The refusal is logged at `info`. |

## Proxy and live settings

Start an HTTP origin and a local proxy in separate terminals. The origin serves a temporary directory.

```sh
origin="$(mktemp -d)"
printf 'Hello from the HTTP tour\n' > "$origin/index.html"
python3 -m http.server 8000 --bind 127.0.0.1 --directory "$origin"
```

```sh
mitmdump --listen-host 127.0.0.1 --listen-port 8080
```

In the terminal with `YARD_HOME` set, write the proxy setting and launch the tour. Clear any proxy exclusions so the loopback origin goes through the proxy.

```sh
printf '%s\n' '{"network":{"proxy":"http://127.0.0.1:8080"}}' > "$YARD_HOME/config/settings.json"
NO_PROXY= no_proxy= HTTP_TOUR=http://127.0.0.1:8000/ cargo run dev
```

Click **Fetch**. The proxy shows the request, and the card shows the origin's response.

Start a second proxy with `mitmdump --listen-host 127.0.0.1 --listen-port 8081`. Keep the app open and update `config/settings.json` in the same temporary home. Wait for `http-configure` before clicking **Fetch** again.

| Settings file | What to look for |
| --- | --- |
| `{"network":{"proxy":"http://127.0.0.1:8081"}}` | The next request appears on the second proxy. |
| `{"network":{"proxy":"not a URL"}}` | The configuration logs a `WARN` and **Fetch** reports a proxy error. Neither the origin nor the proxy receives a request. |
| `{"network":{"proxy":"http://127.0.0.1:8080"}}` | The next fetch succeeds through the proxy. |

## HTTPS_PROXY

Quit the app, remove the explicit proxy setting, then restart the proxy with TLS pass-through for the target host. The proxy can observe the connection while the application verifies the origin's certificate.

```sh
printf '%s\n' '{"network":{}}' > "$YARD_HOME/config/settings.json"
```

```sh
mitmdump --listen-host 127.0.0.1 --listen-port 8080 --ignore-hosts '^example\.com:443$'
```

```sh
HTTPS_PROXY=http://127.0.0.1:8080 NO_PROXY= no_proxy= HTTP_TOUR=https://example.com/ cargo run dev
```

Click **Fetch**. The proxy shows a connection to `example.com:443`, and the card reports the HTTPS response.
