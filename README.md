# EmbyStream

<p align="center">
<a href="https://github.com/Open-PiliPili/EmbyStream">
<img alt="EmbyStream Logo" src="https://raw.githubusercontent.com/Open-PiliPili/EmbyStream/main/res/imgs/logo.jpg" width="400" />
</a>
</p>
<h1 align="center">EmbyStream</h1>
<p align="center">
A Rust-based Emby streaming gateway with a built-in Web Config Studio.
</p>
<p align="center">
<a href="https://t.me/openpilipili_chat"><img src="https://img.shields.io/badge/-Telegram_Group-red?color=blue&logo=telegram&logoColor=white" alt="Telegram"></a>
<a href="https://github.com/open-pilipili/EmbyStream/commit/main"><img src="https://img.shields.io/github/commit-activity/m/open-pilipili/EmbyStream/main" alt="Commit Activity"></a>
<a href="https://github.com/open-pilipili/EmbyStream"><img src="https://img.shields.io/github/languages/top/open-pilipili/EmbyStream" alt="Top Language"></a>
<a href="https://crates.io/crates/embystream"><img src="https://img.shields.io/crates/v/embystream.svg" alt="crates.io"></a>
<a href="https://github.com/open-pilipili/EmbyStream/blob/main/LICENSE"><img src="https://img.shields.io/github/license/open-pilipili/EmbyStream" alt="Github License"></a>
<a href="https://github.com/Open-PiliPili/EmbyStream/actions/workflows/ci-rust.yaml"><img src="https://github.com/Open-PiliPili/EmbyStream/actions/workflows/ci-rust.yaml/badge.svg" alt="CI Rust"></a>
<a href="https://github.com/Open-PiliPili/EmbyStream/actions/workflows/ci-web.yaml"><img src="https://github.com/Open-PiliPili/EmbyStream/actions/workflows/ci-web.yaml/badge.svg" alt="CI Web"></a>
</p>

## What it does

EmbyStream sits between Emby clients and storage backends.

- Web-first setup with local accounts, drafts, generated deployment artifacts, and admin logs
- `frontend`, `backend`, and `dual` gateway modes
- Disk, OpenList, DirectLink, WebDav, Google Drive, and StreamRelay backends
- Signed playback links, path rewrites, anti-hotlinking, and User-Agent filtering

For architecture details and full configuration semantics, use the docs linked below.

## Quick start

Build the Web UI and start the local admin service:

```bash
cd web
bun install
bun run build
cd ..

cargo run -- web serve \
  --listen 127.0.0.1:6888 \
  --data-dir ./web_data \
  --runtime-log-dir ./logs
```

Then open `http://127.0.0.1:6888`.

The Web Config Studio is now the recommended setup path. It can:

- create and manage local admin accounts
- edit config drafts in the browser
- generate `config.toml`, `nginx.conf`, `docker-compose.yaml`, `systemd.service`, and `pm2.config.cjs`
- inspect admin-only logs in the browser

If you set `--tmdb-api-key`, the login page uses TMDB trending backgrounds. Otherwise it falls back to Bing daily images.

## Build and package

Build an embedded single-binary release locally:

```bash
./scripts/build-binary.sh
```

The script builds `web/dist`, compiles the Rust binary, and writes artifacts under `./.build/binary/release/` or `./.build/binary/debug/`.

Build the integrated Docker image locally:

```bash
./scripts/build-docker.sh --tag embystream:latest
```

By default the script writes Docker metadata and local image archives under `.build/docker/`.

For published artifacts, see:

- [GitHub Releases](https://github.com/Open-PiliPili/EmbyStream/releases)
- [Docker Hub - openpilipili/embystream](https://hub.docker.com/r/openpilipili/embystream)
- `ghcr.io/pilipili-team/embystream`

## Install

From crates.io:

```bash
cargo install embystream
```

From source:

```bash
git clone https://github.com/Open-PiliPili/EmbyStream.git
cd EmbyStream
./scripts/build-binary.sh
```

## CLI fallback

The legacy CLI gateway flow is still supported when you want to manage `config.toml` directly:

```bash
embystream config template
embystream run --config ./config.toml
```

Use `embystream --lang zh --help` for Simplified Chinese CLI help and wizard prompts.

## Documentation

| Document | Description |
|----------|-------------|
| [User guide](docs/user-guide.md) | Web-first setup flow, deployment notes, and CLI fallback |
| [CLI usage](docs/cli.md) | `run`, `config`, `auth google`, and `web` commands |
| [Configuration reference](docs/configuration-reference.md) | Full TOML field reference |
| [Google OAuth Desktop App Setup](docs/google-oauth-desktop-app-setup.en.md) | Google Drive OAuth setup |
| [Google OAuth Desktop App 创建教程](docs/google-oauth-desktop-app-setup.zh-CN.md) | Google Drive OAuth 中文教程 |

## License

Copyright (c) 2025 open-pilipili.

EmbyStream is licensed under the [GPL-3.0](https://www.gnu.org/licenses/gpl-3.0.html).
