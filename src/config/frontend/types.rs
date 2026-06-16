use serde::{Deserialize, Serialize};

use crate::config::types::{AntiReverseProxyConfig, PathRewriteConfig};

fn default_check_file_existence() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Frontend {
    pub listen_port: u16,

    #[serde(default = "default_check_file_existence")]
    pub check_file_existence: bool,

    #[serde(default, rename = "PathRewrite")]
    pub path_rewrites: Vec<PathRewriteConfig>,

    #[serde(default, rename = "AntiReverseProxy")]
    pub anti_reverse_proxy: AntiReverseProxyConfig,

    /// Client UA patterns exempted from the `deviceId` check.
    ///
    /// When a request's client identifier (the `Client` header, falling back
    /// to `User-Agent`) matches any pattern in this list, an empty `deviceId`
    /// will **not** be rejected.  Instead, the `emby_token` is used as a
    /// fallback `device_id` value for the redirect URL.
    ///
    /// Matching reuses `UserAgentMatcher::is_ua_matching`, so special rules
    /// for `"emby"` and `"infuse"` apply.
    #[serde(default)]
    pub device_id_exempt_clients: Vec<String>,
}
