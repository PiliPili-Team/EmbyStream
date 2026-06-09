//! Comment-free TOML emission with default key omission.

use serde::Serialize;

use crate::config::{
    backend::{Backend, BackendNode, GoogleDriveConfig, WebDavConfig},
    frontend::Frontend,
    general::StreamMode,
    http2::Http2,
    types::{AntiReverseProxyConfig, PathRewriteConfig, RawConfig},
};
use crate::core::backend::webdav::{DEFAULT_QUERY_PARAM, MODE_PATH_JOIN};

#[derive(Serialize)]
struct EmitHttp2 {
    #[serde(skip_serializing_if = "str::is_empty")]
    ssl_cert_file: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    ssl_key_file: String,
}

#[derive(Serialize)]
struct EmitPathRewrite {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    enable: bool,
    #[serde(skip_serializing_if = "str::is_empty")]
    pattern: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    replacement: String,
}

#[derive(Serialize)]
struct EmitAntiRev {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    enable: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", rename = "host")]
    trusted_hosts: Vec<String>,
}

#[derive(Serialize)]
struct EmitBackendNode {
    name: String,
    #[serde(rename = "type")]
    backend_type: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pattern: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    base_url: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    port: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    path: String,
    #[serde(skip_serializing_if = "is_zero_i32")]
    priority: i32,
    #[serde(skip_serializing_if = "is_default_redirect")]
    proxy_mode: String,
    #[serde(skip_serializing_if = "is_zero_u64")]
    client_speed_limit_kbs: u64,
    #[serde(skip_serializing_if = "is_zero_u64")]
    client_burst_speed_kbs: u64,
    #[serde(rename = "PathRewrite", skip_serializing_if = "Vec::is_empty")]
    path_rewrites: Vec<EmitPathRewrite>,
    #[serde(
        rename = "AntiReverseProxy",
        skip_serializing_if = "Option::is_none"
    )]
    anti_reverse_proxy: Option<EmitAntiRev>,
    #[serde(rename = "Disk", skip_serializing_if = "Option::is_none")]
    disk: Option<crate::config::backend::disk::Disk>,
    #[serde(rename = "OpenList", skip_serializing_if = "Option::is_none")]
    open_list: Option<crate::config::backend::openlist::OpenList>,
    #[serde(rename = "DirectLink", skip_serializing_if = "Option::is_none")]
    direct_link: Option<crate::config::backend::direct::DirectLink>,
    #[serde(rename = "GoogleDrive", skip_serializing_if = "Option::is_none")]
    google_drive: Option<EmitGoogleDrive>,
    #[serde(rename = "WebDav", skip_serializing_if = "Option::is_none")]
    webdav: Option<EmitWebDav>,
}

fn is_zero_i32(n: &i32) -> bool {
    *n == 0
}

fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

fn is_default_redirect(s: &str) -> bool {
    s.is_empty() || s == "redirect"
}

fn map_path_rewrite(p: &PathRewriteConfig) -> EmitPathRewrite {
    EmitPathRewrite {
        enable: p.enable,
        pattern: p.pattern.clone(),
        replacement: p.replacement.clone(),
    }
}

fn map_anti(p: &AntiReverseProxyConfig) -> EmitAntiRev {
    EmitAntiRev {
        enable: p.enable,
        trusted_hosts: p.trusted_hosts.clone(),
    }
}

/// Omit `[*.AntiReverseProxy]` when disabled and no trusted host (matches runtime defaults).
fn anti_reverse_absent(p: &AntiReverseProxyConfig) -> bool {
    !p.enable && p.trusted_hosts.is_empty()
}

fn map_anti_opt(p: &AntiReverseProxyConfig) -> Option<EmitAntiRev> {
    if anti_reverse_absent(p) {
        None
    } else {
        Some(map_anti(p))
    }
}

fn should_omit_webdav_table(w: &WebDavConfig) -> bool {
    let m = w.url_mode.trim();
    let path_join = m.is_empty() || m.eq_ignore_ascii_case(MODE_PATH_JOIN);
    if !path_join {
        return false;
    }
    w.node_uuid.trim().is_empty()
        && w.url_template.trim().is_empty()
        && w.username.trim().is_empty()
        && w.password.trim().is_empty()
        && w.user_agent.trim().is_empty()
        && (w.query_param.trim().is_empty()
            || w.query_param == DEFAULT_QUERY_PARAM)
}

fn skip_webdav_url_mode(s: &str) -> bool {
    s.trim().is_empty() || s.eq_ignore_ascii_case(MODE_PATH_JOIN)
}

fn skip_webdav_query_param(s: &str) -> bool {
    s.trim().is_empty() || s == DEFAULT_QUERY_PARAM
}

fn should_omit_google_drive_table(g: &GoogleDriveConfig) -> bool {
    g.node_uuid.trim().is_empty()
        && g.client_id.trim().is_empty()
        && g.client_secret.trim().is_empty()
        && g.drive_id.trim().is_empty()
        && g.drive_name.trim().is_empty()
        && g.access_token.trim().is_empty()
        && g.refresh_token.trim().is_empty()
}

#[derive(Serialize)]
struct EmitGoogleDrive {
    #[serde(skip_serializing_if = "str::is_empty")]
    node_uuid: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    client_id: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    client_secret: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    drive_id: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    drive_name: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    access_token: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    refresh_token: String,
}

fn map_google_drive_emit(g: &GoogleDriveConfig) -> Option<EmitGoogleDrive> {
    if should_omit_google_drive_table(g) {
        return None;
    }
    Some(EmitGoogleDrive {
        node_uuid: g.node_uuid.clone(),
        client_id: g.client_id.clone(),
        client_secret: g.client_secret.clone(),
        drive_id: g.drive_id.clone(),
        drive_name: g.drive_name.clone(),
        access_token: g.access_token.clone(),
        refresh_token: g.refresh_token.clone(),
    })
}

#[derive(Serialize)]
struct EmitWebDav {
    #[serde(skip_serializing_if = "skip_webdav_url_mode")]
    url_mode: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    node_uuid: String,
    #[serde(skip_serializing_if = "skip_webdav_query_param")]
    query_param: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    url_template: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    username: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    password: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    user_agent: String,
}

fn map_webdav_emit(w: &WebDavConfig) -> Option<EmitWebDav> {
    if should_omit_webdav_table(w) {
        return None;
    }
    Some(EmitWebDav {
        url_mode: w.url_mode.clone(),
        node_uuid: w.node_uuid.clone(),
        query_param: w.query_param.clone(),
        url_template: w.url_template.clone(),
        username: w.username.clone(),
        password: w.password.clone(),
        user_agent: w.user_agent.clone(),
    })
}

fn should_emit_path_rewrite(p: &PathRewriteConfig) -> bool {
    p.enable || !p.pattern.is_empty() || !p.replacement.is_empty()
}

fn map_node(n: &BackendNode) -> EmitBackendNode {
    let path_rewrites: Vec<EmitPathRewrite> = n
        .path_rewrites
        .iter()
        .filter(|p| should_emit_path_rewrite(p))
        .map(map_path_rewrite)
        .collect();
    EmitBackendNode {
        name: n.name.clone(),
        backend_type: n.backend_type.clone(),
        pattern: n.pattern.clone(),
        base_url: n.base_url.clone(),
        port: n.port.clone(),
        path: n.path.clone(),
        priority: n.priority,
        proxy_mode: n.proxy_mode.clone(),
        client_speed_limit_kbs: n.client_speed_limit_kbs,
        client_burst_speed_kbs: n.client_burst_speed_kbs,
        path_rewrites,
        anti_reverse_proxy: map_anti_opt(&n.anti_reverse_proxy),
        disk: n.disk.clone(),
        open_list: n.open_list.clone(),
        direct_link: n.direct_link.clone(),
        google_drive: n.google_drive.as_ref().and_then(map_google_drive_emit),
        webdav: n.webdav.as_ref().and_then(map_webdav_emit),
    }
}

fn map_http2(h: &Http2) -> Option<EmitHttp2> {
    if h.ssl_cert_file.is_empty() && h.ssl_key_file.is_empty() {
        return None;
    }
    Some(EmitHttp2 {
        ssl_cert_file: h.ssl_cert_file.clone(),
        ssl_key_file: h.ssl_key_file.clone(),
    })
}

// --- Wizard / template emission: keep required keys and common defaults visible ---

#[derive(Serialize)]
struct WizardEmitLog {
    level: String,
    prefix: String,
    root_path: String,
}

#[derive(Serialize)]
struct WizardEmitGeneral {
    memory_mode: String,
    stream_mode: StreamMode,
    encipher_key: String,
    encipher_iv: String,
}

#[derive(Serialize)]
struct WizardEmitEmby {
    url: String,
    port: String,
    token: String,
}

#[derive(Serialize)]
struct WizardEmitUserAgent {
    mode: String,
    allow_ua: Vec<String>,
    deny_ua: Vec<String>,
}

#[derive(Serialize)]
struct WizardEmitAntiRev {
    enable: bool,
    #[serde(rename = "host")]
    trusted_hosts: Vec<String>,
}

#[derive(Serialize)]
struct WizardEmitFrontend {
    listen_port: u16,
    check_file_existence: bool,
    #[serde(rename = "PathRewrite", skip_serializing_if = "Vec::is_empty")]
    path_rewrites: Vec<EmitPathRewrite>,
    #[serde(
        rename = "AntiReverseProxy",
        skip_serializing_if = "Option::is_none"
    )]
    anti_reverse_proxy: Option<WizardEmitAntiRev>,
}

#[derive(Serialize)]
struct WizardEmitBackend {
    listen_port: u16,
    base_url: String,
    port: String,
    path: String,
    check_file_existence: bool,
    problematic_clients: Vec<String>,
}

#[derive(Serialize)]
struct WizardEmitFallback {
    video_missing_path: String,
}

#[derive(Serialize)]
struct WizardEmitDoc {
    #[serde(rename = "Log")]
    log: WizardEmitLog,
    #[serde(rename = "General")]
    general: WizardEmitGeneral,
    #[serde(rename = "Emby")]
    emby: WizardEmitEmby,
    #[serde(rename = "UserAgent")]
    user_agent: WizardEmitUserAgent,
    #[serde(rename = "Http2", skip_serializing_if = "Option::is_none")]
    http2: Option<EmitHttp2>,
    #[serde(rename = "Frontend", skip_serializing_if = "Option::is_none")]
    frontend: Option<WizardEmitFrontend>,
    #[serde(rename = "Backend", skip_serializing_if = "Option::is_none")]
    backend: Option<WizardEmitBackend>,
    #[serde(rename = "BackendNode", skip_serializing_if = "Vec::is_empty")]
    backend_nodes: Vec<EmitBackendNode>,
    #[serde(rename = "Fallback")]
    fallback: WizardEmitFallback,
}

fn map_anti_wizard_opt(
    p: &AntiReverseProxyConfig,
) -> Option<WizardEmitAntiRev> {
    if anti_reverse_absent(p) {
        None
    } else {
        Some(WizardEmitAntiRev {
            enable: p.enable,
            trusted_hosts: p.trusted_hosts.clone(),
        })
    }
}

fn map_frontend_wizard(f: &Frontend) -> WizardEmitFrontend {
    let path_rewrites: Vec<EmitPathRewrite> = f
        .path_rewrites
        .iter()
        .filter(|p| should_emit_path_rewrite(p))
        .map(map_path_rewrite)
        .collect();
    WizardEmitFrontend {
        listen_port: f.listen_port,
        check_file_existence: f.check_file_existence,
        path_rewrites,
        anti_reverse_proxy: map_anti_wizard_opt(&f.anti_reverse_proxy),
    }
}

fn map_backend_wizard(b: &Backend) -> WizardEmitBackend {
    WizardEmitBackend {
        listen_port: b.listen_port,
        base_url: b.base_url.clone(),
        port: b.port.clone(),
        path: b.path.clone(),
        check_file_existence: b.check_file_existence,
        problematic_clients: b.problematic_clients.clone(),
    }
}

/// Serialize to comment-free TOML with typical defaults kept visible (wizard / template).
pub fn emit_wizard_config_toml(
    raw: &RawConfig,
) -> Result<String, toml::ser::Error> {
    let http2 = raw.http2.as_ref().and_then(map_http2);
    let backend_nodes: Vec<EmitBackendNode> = raw
        .backend_nodes
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(map_node)
        .collect();
    let doc = WizardEmitDoc {
        log: WizardEmitLog {
            level: raw.log.level.clone(),
            prefix: raw.log.prefix.clone(),
            root_path: raw.log.root_path.clone(),
        },
        general: WizardEmitGeneral {
            memory_mode: raw.general.memory_mode.clone(),
            stream_mode: raw.general.stream_mode,
            encipher_key: raw.general.encipher_key.clone(),
            encipher_iv: raw.general.encipher_iv.clone(),
        },
        emby: WizardEmitEmby {
            url: raw.emby.url.clone(),
            port: raw.emby.port.clone(),
            token: raw.emby.token.clone(),
        },
        user_agent: WizardEmitUserAgent {
            mode: raw.user_agent.mode.clone(),
            allow_ua: raw.user_agent.allow_ua.clone(),
            deny_ua: raw.user_agent.deny_ua.clone(),
        },
        http2,
        frontend: raw.frontend.as_ref().map(map_frontend_wizard),
        backend: raw.backend.as_ref().map(map_backend_wizard),
        backend_nodes,
        fallback: WizardEmitFallback {
            video_missing_path: raw.fallback.video_missing_path.clone(),
        },
    };
    toml::to_string_pretty(&doc)
}

#[cfg(test)]
pub(crate) mod compact_emit_test {
    use super::{
        EmitAntiRev, EmitBackendNode, EmitHttp2, EmitPathRewrite, Frontend,
        RawConfig, map_anti, map_http2, map_node, map_path_rewrite,
        should_emit_path_rewrite,
    };
    use crate::config::{backend::Backend, general::StreamMode};
    use serde::Serialize;

    #[derive(Serialize)]
    struct EmitLog {
        #[serde(skip_serializing_if = "is_default_info")]
        level: String,
        #[serde(skip_serializing_if = "str::is_empty")]
        prefix: String,
        #[serde(skip_serializing_if = "is_default_logs_root")]
        root_path: String,
    }

    #[derive(Serialize)]
    struct EmitGeneral {
        #[serde(skip_serializing_if = "is_default_memory")]
        memory_mode: String,
        stream_mode: StreamMode,
        #[serde(skip_serializing_if = "str::is_empty")]
        encipher_key: String,
        #[serde(skip_serializing_if = "str::is_empty")]
        encipher_iv: String,
    }

    #[derive(Serialize)]
    struct EmitEmby {
        #[serde(skip_serializing_if = "str::is_empty")]
        url: String,
        #[serde(skip_serializing_if = "str::is_empty")]
        port: String,
        #[serde(skip_serializing_if = "str::is_empty")]
        token: String,
    }

    #[derive(Serialize)]
    struct EmitUserAgent {
        #[serde(skip_serializing_if = "is_default_allow_mode")]
        mode: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        allow_ua: Vec<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        deny_ua: Vec<String>,
    }

    #[derive(Serialize)]
    struct EmitFallback {
        #[serde(skip_serializing_if = "str::is_empty")]
        video_missing_path: String,
    }

    #[derive(Serialize)]
    struct EmitFrontend {
        listen_port: u16,
        check_file_existence: bool,
        #[serde(rename = "PathRewrite", skip_serializing_if = "Vec::is_empty")]
        path_rewrites: Vec<EmitPathRewrite>,
        #[serde(rename = "AntiReverseProxy")]
        anti_reverse_proxy: EmitAntiRev,
    }

    #[derive(Serialize)]
    struct EmitBackend {
        listen_port: u16,
        #[serde(skip_serializing_if = "str::is_empty")]
        base_url: String,
        #[serde(skip_serializing_if = "str::is_empty")]
        port: String,
        #[serde(skip_serializing_if = "str::is_empty")]
        path: String,
        check_file_existence: bool,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        problematic_clients: Vec<String>,
    }

    #[derive(Serialize)]
    struct EmitDoc {
        #[serde(rename = "Log")]
        log: EmitLog,
        #[serde(rename = "General")]
        general: EmitGeneral,
        #[serde(rename = "Emby")]
        emby: EmitEmby,
        #[serde(rename = "UserAgent")]
        user_agent: EmitUserAgent,
        #[serde(rename = "Http2", skip_serializing_if = "Option::is_none")]
        http2: Option<EmitHttp2>,
        #[serde(rename = "Frontend", skip_serializing_if = "Option::is_none")]
        frontend: Option<EmitFrontend>,
        #[serde(rename = "Backend", skip_serializing_if = "Option::is_none")]
        backend: Option<EmitBackend>,
        #[serde(rename = "BackendNode", skip_serializing_if = "Vec::is_empty")]
        backend_nodes: Vec<EmitBackendNode>,
        #[serde(rename = "Fallback")]
        fallback: EmitFallback,
    }

    fn is_default_info(s: &str) -> bool {
        s.is_empty() || s == "info"
    }

    fn is_default_logs_root(s: &str) -> bool {
        s.is_empty() || s == "./logs"
    }

    fn is_default_memory(s: &str) -> bool {
        s.is_empty() || s == "middle"
    }

    fn is_default_allow_mode(s: &str) -> bool {
        s == "allow"
    }

    fn map_frontend(f: &Frontend) -> EmitFrontend {
        let path_rewrites: Vec<EmitPathRewrite> = f
            .path_rewrites
            .iter()
            .filter(|p| should_emit_path_rewrite(p))
            .map(map_path_rewrite)
            .collect();
        EmitFrontend {
            listen_port: f.listen_port,
            check_file_existence: f.check_file_existence,
            path_rewrites,
            anti_reverse_proxy: map_anti(&f.anti_reverse_proxy),
        }
    }

    fn map_backend(b: &Backend) -> EmitBackend {
        EmitBackend {
            listen_port: b.listen_port,
            base_url: b.base_url.clone(),
            port: b.port.clone(),
            path: b.path.clone(),
            check_file_existence: b.check_file_existence,
            problematic_clients: b.problematic_clients.clone(),
        }
    }

    /// Compact TOML: omit keys that match defaults (unit tests only).
    pub(crate) fn emit_raw_config_toml(
        raw: &RawConfig,
    ) -> Result<String, toml::ser::Error> {
        let http2 = raw.http2.as_ref().and_then(map_http2);
        let backend_nodes: Vec<EmitBackendNode> = raw
            .backend_nodes
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(map_node)
            .collect();
        let doc = EmitDoc {
            log: EmitLog {
                level: raw.log.level.clone(),
                prefix: raw.log.prefix.clone(),
                root_path: raw.log.root_path.clone(),
            },
            general: EmitGeneral {
                memory_mode: raw.general.memory_mode.clone(),
                stream_mode: raw.general.stream_mode,
                encipher_key: raw.general.encipher_key.clone(),
                encipher_iv: raw.general.encipher_iv.clone(),
            },
            emby: EmitEmby {
                url: raw.emby.url.clone(),
                port: raw.emby.port.clone(),
                token: raw.emby.token.clone(),
            },
            user_agent: EmitUserAgent {
                mode: raw.user_agent.mode.clone(),
                allow_ua: raw.user_agent.allow_ua.clone(),
                deny_ua: raw.user_agent.deny_ua.clone(),
            },
            http2,
            frontend: raw.frontend.as_ref().map(map_frontend),
            backend: raw.backend.as_ref().map(map_backend),
            backend_nodes,
            fallback: EmitFallback {
                video_missing_path: raw.fallback.video_missing_path.clone(),
            },
        };
        toml::to_string_pretty(&doc)
    }
}
