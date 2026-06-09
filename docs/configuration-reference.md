# Configuration reference

EmbyStream reads a single TOML file. The Web Config Studio generates the same structure described here, so this document is the reference for both web-generated configs and hand-edited `config.toml` files.

Table names match the shipped template: [`src/config/config.toml.template`](../src/config/config.toml.template). Optional CLI overrides exist for TLS paths (see [CLI usage](cli.md)).

---

## `[Log]`

| Field       | Type   | Default   | Description |
|------------|--------|-----------|-------------|
| `level`    | string | `"info"`  | Log verbosity (e.g. `trace`, `debug`, `info`, `warn`, `error`). |
| `prefix`   | string | `""`      | Prefix for rotated log file names. |
| `root_path`| string | `"./logs"`| Directory created at startup; log files are written here. |

**Example — production host with central log path**

```toml
[Log]
level = "info"
prefix = "embystream"
root_path = "/var/log/embystream"
```

**Example — verbose troubleshooting**

```toml
[Log]
level = "debug"
root_path = "./logs"
```

---

## `[General]`

| Field           | Type   | Description |
|----------------|--------|-------------|
| `memory_mode`  | string | Cache sizing hint: `low`, `middle` (default), or `high`. Affects in-memory cache capacity and TTL used with backend rate limiting. |
| `stream_mode`  | string | `frontend`, `backend`, or `dual`. Controls which gateways start. `dual` requires **different** `listen_port` values for frontend and backend. |
| `encipher_key` | string | Secret key for sign encryption (change from template). |
| `encipher_iv`  | string | IV for sign encryption (change from template). |

Signed playback URLs embed an encrypted payload; use strong, unique `encipher_key` / `encipher_iv` in any network-exposed deployment.

**Example — frontend-only reverse proxy**

```toml
[General]
memory_mode = "middle"
stream_mode = "frontend"
encipher_key = "YOUR_KEY"
encipher_iv = "YOUR_IV"
```

**Example — backend-only stream gateway**

```toml
[General]
stream_mode = "backend"
memory_mode = "high"
encipher_key = "YOUR_KEY"
encipher_iv = "YOUR_IV"
```

**Example — dual gateways (typical split deployment)**

```toml
[General]
stream_mode = "dual"
memory_mode = "middle"
encipher_key = "YOUR_KEY"
encipher_iv = "YOUR_IV"
```

Ensure `[Frontend].listen_port` ≠ `[Backend].listen_port` (e.g. `60001` and `60002`).

---

## `[Emby]`

Used when `stream_mode` is `frontend` or `dual`. The frontend gateway reverse-proxies to this Emby base URL and uses the API token where needed.

| Field   | Type   | Description |
|---------|--------|-------------|
| `url`   | string | Base URL without trailing slash (e.g. `http://127.0.0.1` or `https://emby.example.com`). |
| `port`  | string | Emby port; omitted in the built URI when `80` or `443`. |
| `token` | string | Emby API access token. |

**Example — local Emby**

```toml
[Emby]
url = "http://127.0.0.1"
port = "8096"
token = "YOUR_EMBY_API_KEY"
```

**Example — HTTPS Emby on 443**

```toml
[Emby]
url = "https://emby.home"
port = "443"
token = "YOUR_EMBY_API_KEY"
```

---

## `[UserAgent]`

Gateway-wide User-Agent filtering (frontend uses additional path-based rules from compiled defaults where applicable).

| Field     | Type       | Description |
|-----------|------------|-------------|
| `mode`    | string     | `allow` (default): only listed agents pass. `deny`: listed agents are blocked. |
| `allow_ua`| string array | Substrings matched when `mode = "allow"`. |
| `deny_ua` | string array | Substrings matched when `mode = "deny"`. |

**Example — deny common scraping tools**

```toml
[UserAgent]
mode = "deny"
deny_ua = ["curl", "wget", "python-requests"]
```

**Example — allow only known clients**

```toml
[UserAgent]
mode = "allow"
allow_ua = ["Emby", "Infuse", "SenPlayer"]
```

---

## `[Http2]`

TLS certificate paths for the **backend** listener (HTTPS). Empty strings resolve to files next to the config: `ssl/ssl-cert` and `ssl/ssl-key` under the config directory.

| Field          | Type   | Description |
|----------------|--------|-------------|
| `ssl_cert_file`| string | PEM certificate path (absolute or relative to config dir). |
| `ssl_key_file` | string | PEM private key path. |

You can override these at runtime with `embystream run --ssl-cert-file` / `--ssl-key-file` (see [CLI](cli.md)).

**Example — explicit PEM paths**

```toml
[Http2]
ssl_cert_file = "/etc/embystream/ssl/fullchain.pem"
ssl_key_file = "/etc/embystream/ssl/privkey.pem"
```

---

## `[Fallback]`

| Field                | Type   | Description |
|----------------------|--------|-------------|
| `video_missing_path` | string | Optional local file path served when a requested video is missing (empty disables). |

**Example — placeholder video**

```toml
[Fallback]
video_missing_path = "/srv/media/fallback/placeholder.mp4"
```

---

## `[Frontend]`

Required when `stream_mode` is `frontend` or `dual`.

| Field                    | Type   | Description |
|--------------------------|--------|-------------|
| `listen_port`            | u16    | HTTP port for the frontend gateway. |
| `check_file_existence`   | bool   | When enabled, validates media paths against Emby before forwarding. |

### `[[Frontend.PathRewrite]]`

Ordered rules: first matching enabled rule rewrites the path (regex `pattern` → `replacement`).

**Example — strip a path prefix for CDN**

```toml
[Frontend]
listen_port = 60001
check_file_existence = true

[[Frontend.PathRewrite]]
enable = true
pattern = "^/media(/.*)$"
replacement = "$1"
```

### `[Frontend.AntiReverseProxy]`

| Field    | Type   | Description |
|----------|--------|-------------|
| `enable` | bool   | Reject requests whose `Host` matches none of the trusted hosts. |
| `host`   | string \| string[] | Trusted host(s) (hostname only; scheme stripped if present). Accepts a single string or a list of domains. A request is allowed when its `Host` matches any entry. A leading `*.` makes an entry a wildcard. |

**Example — only accept your public domain**

```toml
[Frontend.AntiReverseProxy]
enable = true
host = "stream.example.com"
```

**Example — accept multiple domains**

```toml
[Frontend.AntiReverseProxy]
enable = true
host = ["a.example.com", "b.example.com"]
```

**Example — wildcard subdomains**

```toml
[Frontend.AntiReverseProxy]
enable = true
host = ["example.com", "*.example.com"]
```

`*.example.com` matches any subdomain at any depth (`a.example.com`,
`x.y.example.com`) but **not** the apex `example.com` — list the apex explicitly
when you also want it allowed.

The legacy single-string form is still accepted and is treated as a one-element
list, so existing configs keep working unchanged.

---

## `[Backend]`

Required when `stream_mode` is `backend` or `dual`.

| Field                  | Type           | Description |
|------------------------|----------------|-------------|
| `listen_port`          | u16            | HTTPS port for the backend gateway. |
| `base_url`             | string         | Public base URL clients use (scheme + host). |
| `port`                 | string         | Public port if not 80/443. |
| `path`                 | string         | URL path segment for the stream service (no leading slash required). |
| `check_file_existence` | bool           | When true, backend local-path routing probes file existence before streaming or applying fallback. Default `true`. |
| `problematic_clients`  | string array   | Client identifiers (substrings) that skip certain optimizations (see logs / code for behavior). |

**Example**

```toml
[Backend]
listen_port = 60002
base_url = "https://stream.example.com"
port = "443"
path = "stream"
check_file_existence = true
problematic_clients = []
```

---

## `[[BackendNode]]`

Each node describes a storage backend. The `type` field selects the integration (case-insensitive matching is used in code).

Common fields:

| Field                      | Type   | Description |
|----------------------------|--------|-------------|
| `name`                     | string | Display name. |
| `type`                     | string | `Disk`, `OpenList`, `DirectLink`, `googleDrive`, `WebDav`, or `StreamRelay`. |
| `pattern`                  | string | If non-empty, must be valid **regex**: for normal nodes it matches the decrypted Emby file path; for `StreamRelay` it matches the **HTTP** request path. If empty, matching falls back to `path` or a catch-all (see code). |
| `base_url`, `port`, `path` | strings| Upstream base URI parts (see template). |
| `priority`                 | i32    | Ordering where applicable (e.g. StreamRelay nodes). |
| `proxy_mode`               | string | `redirect`, `proxy`, or `accel_redirect` (`WebDav` and `googleDrive`) — how responses are delivered to clients. |
| `client_speed_limit_kbs`   | u64    | Per-device speed limit (0 = unlimited). |
| `client_burst_speed_kbs`   | u64    | Burst allowance for the limiter. |

### `Disk` — local or mounted library

```toml
[[BackendNode]]
name = "NAS"
type = "Disk"
pattern = "/mnt/media/.*"
base_url = "http://127.0.0.1"
port = "60002"
path = ""
priority = 0
proxy_mode = "proxy"
client_speed_limit_kbs = 0
client_burst_speed_kbs = 0
```

### `OpenList` — OpenList / Alist

Requires `[BackendNode.OpenList]` with `base_url` and `token`.

```toml
[[BackendNode]]
name = "Alist"
type = "OpenList"
pattern = "/openlist/.*"
base_url = "http://127.0.0.1"
port = "5244"
path = "openlist"
priority = 0
proxy_mode = "redirect"

[BackendNode.OpenList]
base_url = "http://127.0.0.1:5244"
token = "YOUR_OPENLIST_TOKEN"
```

### `DirectLink` — signed or direct HTTP URLs

Optional `[BackendNode.DirectLink]` with `user_agent` for upstream requests.

```toml
[[BackendNode]]
name = "CDN"
type = "DirectLink"
pattern = "/cloud/.*"
base_url = "https://storage.example.com"
port = "443"
path = "media"
proxy_mode = "redirect"

[BackendNode.DirectLink]
user_agent = "EmbyStream/1.0"
```

### `WebDav`

Optional `[BackendNode.WebDav]`:

| Field          | Description |
|----------------|-------------|
| `node_uuid`    | Required when `proxy_mode = "accel_redirect"`. Used to build `X-Accel-Redirect: /_origin/webdav/<node_uuid>/<file_path>`. |
| `url_mode`     | `path_join`, `query_path`, or `url_template`. |
| `query_param`  | Query name when `url_mode = query_path` (default `path`). |
| `url_template` | Template with `{file_path}` when `url_mode = url_template`. |
| `username` / `password` | Basic auth when needed. |
| `user_agent`   | Custom UA for WebDAV HTTP calls. |

`accel_redirect` is intended for Nginx `X-Accel-Redirect` deployments. It is
validated at startup and is only allowed on `WebDav` or `googleDrive` nodes.
When enabled, `node_uuid` must be unique across all `WebDav + accel_redirect`
nodes.

### `googleDrive`

Requires `[BackendNode.GoogleDrive]`:

| Field | Description |
|-------|-------------|
| `node_uuid` | Required. Stable ID used for token/cache keys; must be unique across `googleDrive` nodes. |
| `client_id` | Required OAuth client ID used when refreshing `access_token`. |
| `client_secret` | Required OAuth client secret used when refreshing `access_token`. |
| `drive_id` | Preferred shared drive ID. Takes precedence over `drive_name`. |
| `drive_name` | Shared drive name fallback when `drive_id` is absent. |
| `access_token` | Cached OAuth access token. Can be refreshed and written back by the app later. |
| `refresh_token` | Required OAuth refresh token used to renew `access_token`. |
| `token` | Preferred persisted OAuth token blob. When present, EmbyStream reads `access_token`, `refresh_token`, `token_type`, and `expiry` from it. |

`drive_id` and `drive_name` may both be empty. In that case, runtime will infer the
shared drive name from the first path segment after path rewrite. `proxy_mode=redirect`
is supported but may expose OAuth bearer tokens to clients, so it should be used only
when that leakage risk is acceptable.

When at least one `googleDrive` node is configured, EmbyStream also starts:

- a startup prewarm pass
- an expiry-driven background pre-refresh scheduler

The scheduler reuses the same request-time token source and single-flight
refresh path as normal traffic. It reads persisted expiry, refreshes only when
the remaining lifetime falls below the internal lead window, and is only an
optimization. Request-time token acquisition remains the correctness guarantee
for sparse traffic, restart recovery, and refresh failures.

Recommended delivery modes for `googleDrive`:

- `proxy`: safest default. The server keeps the OAuth bearer token and
  fetches Google Drive on behalf of the client.
- `accel_redirect`: recommended when you already deploy behind Nginx and
  want to offload the large body transfer there.
- `redirect`: supported, but the response includes
  `Authorization: Bearer ...`, so clients or intermediate proxies may
  see the token.

Example `googleDrive` node:

```toml
[[BackendNode]]
name = "GoogleDriveMedia"
type = "googleDrive"
pattern = "^/mnt/media/.*"
proxy_mode = "accel_redirect"

[BackendNode.GoogleDrive]
node_uuid = "google-drive-media"
client_id = "your-google-oauth-client-id"
client_secret = "your-google-oauth-client-secret"
drive_name = "pilipili"
access_token = ""
refresh_token = "your-google-refresh-token"
```

Example Nginx wiring for `googleDrive + accel_redirect`:

```nginx
location /stream {
    proxy_pass http://127.0.0.1:60001;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
}

location ~ ^/_origin/google-drive/([^/]+)/([^/]+)$ {
    internal;

    set $google_node_uuid $1;
    set $google_file_id $2;
    set $google_drive_query "alt=media&supportsAllDrives=true&acknowledgeAbuse=true";
    proxy_pass https://www.googleapis.com/drive/v3/files/$google_file_id?$google_drive_query;
    proxy_set_header Authorization "Bearer $arg_token";
    proxy_set_header Host www.googleapis.com;
    proxy_ssl_server_name on;
}
```

Notes for the Nginx example:

- EmbyStream responds with
  `X-Accel-Redirect: /_origin/google-drive/<node_uuid>/<file_id>?token=Bearer%20...`.
- The internal Nginx location must copy the internal-use query parameter into
  the upstream `Authorization` header, otherwise Google Drive will reject the
  media request.
- `redirect` mode skips this internal hop entirely and therefore carries the
  highest bearer-token exposure risk.

### `StreamRelay`

Redirects matching GET requests to another backend URL **without** decrypting the `sign` parameter — useful for chaining gateways.

```toml
[[BackendNode]]
name = "RelayToEdge"
type = "StreamRelay"
pattern = "^/stream$"
base_url = "https://edge.example.com"
port = "443"
path = "stream"
priority = 0
proxy_mode = "redirect"
```

### Per-node `[[BackendNode.PathRewrite]]` and `[BackendNode.AntiReverseProxy]`

Same semantics as the frontend tables, applied in the backend pipeline for that node.

---

## Config discovery

- **Explicit:** `embystream run --config /path/to/config.toml`
- **Docker:** default path `/config/embystream/config.toml` if present.
- **Otherwise:** OS config directory (e.g. `~/.config/embystream/config.toml` on Unix), with a template copy on first run when the template file is available next to the binary working directory (development).

---

## Related

- [User guide](user-guide.md) — deployment scenarios and Emby URL layout.
- [CLI usage](cli.md) — flags and config wizard.
