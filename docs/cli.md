# CLI usage

The binary name is `embystream`.

For first-time setup, the recommended entry point is still `embystream web serve` plus the browser-based Web Config Studio. The CLI configuration flow remains available for direct TOML management.

---

## Global behavior

```text
embystream [COMMAND]
```

- `embystream --help` prints the top-level command list.
- `embystream --version` prints the version.
- `embystream` without a subcommand exits without starting any service.

## Language (`--lang`)

| Value | Effect |
|-------|--------|
| `en` | English help and wizard prompts. |
| `zh` | Simplified Chinese top-level help and wizard prompts. |

Examples:

```bash
embystream --lang zh --help
embystream --lang zh config template --help
```

---

## `embystream web`

Starts the Web Config Studio or performs web-admin maintenance tasks.

### `embystream web serve`

```bash
embystream web serve \
  --listen 127.0.0.1:6888 \
  --data-dir ./web_data \
  --runtime-log-dir ./logs
```

Options:

| Option | Description |
|--------|-------------|
| `--listen <ADDR>` | Web service listen address. Default `0.0.0.0:6888`. |
| `--data-dir <DIR>` | SQLite data, sessions, generated artifacts, and audit-log state. Default `./web_data`. |
| `--runtime-log-dir <DIR>` | Directory used for runtime log persistence and browser log replay. Default `./logs`. |
| `--tmdb-api-key <KEY>` | Optional TMDB API key for login backgrounds. |

Behavior:

- serves the Rust JSON API
- serves frontend assets from embedded resources or `web/dist` when available
- exposes admin-only browser logs, drafts, generated artifacts, and local account management
- falls back to Bing login backgrounds when TMDB is not configured

To build a self-contained local binary with embedded frontend assets, use:

```bash
./scripts/build-binary.sh
```

The default output root is `./.build`, with binaries written under `.build/binary/release/` or `.build/binary/debug/`.

### `embystream web admin reset-password`

```bash
embystream web admin reset-password \
  --data-dir ./web_data \
  --username admin
```

Behavior:

- resets the target admin password
- prints the new random password once to stdout
- does not provide browser-based password recovery

---

## `embystream run`

Starts the gateway process from a `config.toml` file.

| Mode | What starts |
|------|-------------|
| `frontend` | Frontend reverse proxy only |
| `backend` | Backend stream gateway only |
| `dual` | Both frontend and backend in one process |

Options:

| Option | Description |
|--------|-------------|
| `-c, --config <FILE>` | Path to `config.toml`. |
| `--web` | Also start the Web Config Studio alongside the gateway. |
| `--ssl-cert-file <FILE>` | Override `[Http2].ssl_cert_file` for this process. |
| `--ssl-key-file <FILE>` | Override `[Http2].ssl_key_file` for this process. |

Examples:

```bash
embystream run
embystream run --config /etc/embystream/config.toml
embystream run --config ./config.toml \
  --ssl-cert-file /run/secrets/cert.pem \
  --ssl-key-file /run/secrets/key.pem
```

Use this path when you intentionally want the legacy CLI gateway workflow instead of the Web Config Studio.

### Toggling the Web Config Studio with an environment variable

The `WEB_ENABLE` environment variable overrides the `--web` flag, so deployments
can turn the studio on or off without editing the command line. This is the
recommended switch for the Docker image, whose default command bakes in `--web`.

| `WEB_ENABLE` | Effect |
|--------------|--------|
| unset (or unrecognized value) | Falls back to the `--web` flag. |
| `1`, `true`, `yes`, `on`, `enable`, `enabled` | Force the studio **on**. |
| `0`, `false`, `no`, `off`, `disable`, `disabled` | Force the studio **off**. |

Values are case-insensitive and surrounding whitespace is ignored. The lower-case
alias `web_enable` is also accepted.

```bash
# Binary: start the gateway without the studio even if a wrapper adds --web
WEB_ENABLE=false embystream run --config ./config.toml --web

# Docker: disable the studio without overriding the image command
docker run -e WEB_ENABLE=false openpilipili/embystream:latest
```

---

## `embystream config`

Interactive TOML-focused helpers.

### `embystream config template`

Creates a starter config through the terminal wizard and writes it atomically.

```bash
embystream config template
```

Use this when you want to bootstrap a config without the web admin.

### `embystream config show`

Scans the working directory for valid TOML configs and prints the selected one with secrets masked unless you confirm otherwise.

```bash
embystream config show
```

---

## `embystream auth google`

Starts a Google OAuth installed-app flow for Drive read-only access.

```bash
embystream auth google \
  --client-id YOUR_CLIENT_ID \
  --secret YOUR_CLIENT_SECRET
```

Behavior:

- prints the authorization URL
- opens a browser by default
- spins up a localhost callback
- prints `access_token`, `refresh_token`, and `expires_at` after success

Use `--no-browser` on headless hosts:

```bash
embystream auth google \
  --client-id YOUR_CLIENT_ID \
  --secret YOUR_CLIENT_SECRET \
  --no-browser
```

---

## Build helpers

These repository scripts mirror the current local packaging paths:

```bash
./scripts/build-binary.sh
./scripts/build-docker.sh --tag embystream:latest
```

Docker metadata and local image archives are written under `./.build/docker/`.

---

## Related

- [User guide](user-guide.md)
- [Configuration reference](configuration-reference.md)
- [Google OAuth Desktop App Setup (EN)](google-oauth-desktop-app-setup.en.md)
- [Google OAuth Desktop App 创建教程 (ZH-CN)](google-oauth-desktop-app-setup.zh-CN.md)
