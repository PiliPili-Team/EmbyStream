use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::config::{
    backend::{
        Backend, BackendNode, direct::DirectLink, disk::Disk,
        openlist::OpenList,
    },
    frontend::Frontend,
    general::{Emby, General, Log, UserAgent},
    http2::Http2,
};

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct PathRewriteConfig {
    #[serde(default)]
    pub enable: bool,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub replacement: String,
}

impl PathRewriteConfig {
    pub fn is_need_rewrite(&self, path: &str) -> bool {
        if path.is_empty() || !self.enable {
            return false;
        }
        !self.pattern.is_empty() && !self.replacement.is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AntiReverseProxyConfig {
    #[serde(default)]
    pub enable: bool,
    /// Trusted hosts allowed to access the proxied service.
    ///
    /// Accepts both the legacy single-string form (`host = "a.example.com"`)
    /// and the multi-domain list form (`host = ["a.com", "b.com"]`) on read;
    /// the single string is normalized into a one-element vector. On write the
    /// value is always serialized as a list for forward compatibility.
    #[serde(
        default,
        rename = "host",
        deserialize_with = "deserialize_trusted_hosts",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub trusted_hosts: Vec<String>,
}

/// Normalizes a host candidate into a bare hostname for comparison.
///
/// Strips an optional scheme, then keeps only the authority component before
/// any path or port. Returns `None` when the candidate is empty.
fn extract_valid_host(url: &str) -> Option<&str> {
    let cleaned = url
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    cleaned
        .split(['/', ':'])
        .next()
        .filter(|&s| !s.is_empty())
        .map(|s| s.trim_end_matches('/'))
}

/// Deserializes the `host` field from either a single string or a list of
/// strings, trimming whitespace and dropping empty entries.
fn deserialize_trusted_hosts<'de, D>(
    deserializer: D,
) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct TrustedHostsVisitor;

    impl<'de> Visitor<'de> for TrustedHostsVisitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a host string or a list of host strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let trimmed = value.trim();
            Ok(if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            })
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            self.visit_str(&value)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut hosts = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                let trimmed = item.trim();
                if !trimmed.is_empty() {
                    hosts.push(trimmed.to_string());
                }
            }
            Ok(hosts)
        }
    }

    deserializer.deserialize_any(TrustedHostsVisitor)
}

impl AntiReverseProxyConfig {
    /// Returns `true` when the request `host` should be blocked because it does
    /// not match any configured trusted host.
    ///
    /// Disabled config, an empty trusted list, or an unparseable request host
    /// all yield `false` (request allowed) to preserve fail-open behavior.
    #[inline]
    pub fn is_need_anti(&self, host: &str) -> bool {
        if !self.enable || self.trusted_hosts.is_empty() {
            return false;
        }

        let Some(request_host) = extract_valid_host(host) else {
            return false;
        };

        let matches_trusted = self.trusted_hosts.iter().any(|trusted| {
            extract_valid_host(trusted)
                .is_some_and(|t| t.eq_ignore_ascii_case(request_host))
        });

        !matches_trusted
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct FallbackConfig {
    #[serde(default)]
    pub video_missing_path: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct RawConfig {
    #[serde(rename = "General")]
    pub general: General,
    #[serde(rename = "Log")]
    pub log: Log,
    #[serde(rename = "Emby")]
    pub emby: Emby,
    #[serde(rename = "UserAgent")]
    pub user_agent: UserAgent,
    #[serde(rename = "Http2")]
    pub http2: Option<Http2>,
    #[serde(rename = "Frontend")]
    pub frontend: Option<Frontend>,
    #[serde(rename = "Backend")]
    pub backend: Option<Backend>,
    #[serde(rename = "BackendNode")]
    pub backend_nodes: Option<Vec<BackendNode>>,
    #[serde(rename = "Disk")]
    pub disk: Option<Disk>,
    #[serde(rename = "OpenList")]
    pub open_list: Option<OpenList>,
    #[serde(rename = "DirectLink")]
    pub direct_link: Option<DirectLink>,
    #[serde(rename = "Fallback", default)]
    pub fallback: FallbackConfig,
}

#[cfg(test)]
mod anti_reverse_proxy_tests {
    use super::AntiReverseProxyConfig;

    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(rename = "AntiReverseProxy")]
        anti: AntiReverseProxyConfig,
    }

    fn parse(toml_str: &str) -> AntiReverseProxyConfig {
        toml::from_str::<Wrapper>(toml_str)
            .expect("anti reverse proxy config should parse")
            .anti
    }

    #[test]
    fn legacy_single_string_host_becomes_one_element_vec() {
        let config = parse(
            r#"
            [AntiReverseProxy]
            enable = true
            host = "a.example.com"
            "#,
        );

        assert_eq!(config.trusted_hosts, vec!["a.example.com".to_string()]);
    }

    #[test]
    fn list_form_keeps_all_hosts_and_trims_blanks() {
        let config = parse(
            r#"
            [AntiReverseProxy]
            enable = true
            host = ["a.example.com", "  b.example.com  ", ""]
            "#,
        );

        assert_eq!(
            config.trusted_hosts,
            vec!["a.example.com".to_string(), "b.example.com".to_string()]
        );
    }

    #[test]
    fn missing_host_defaults_to_empty_vec() {
        let config = parse(
            r#"
            [AntiReverseProxy]
            enable = true
            "#,
        );

        assert!(config.trusted_hosts.is_empty());
    }

    #[test]
    fn any_trusted_host_passes_with_multiple_domains() {
        let config = parse(
            r#"
            [AntiReverseProxy]
            enable = true
            host = ["a.example.com", "b.example.com"]
            "#,
        );

        // Both configured domains are allowed (not blocked).
        assert!(!config.is_need_anti("a.example.com"));
        assert!(!config.is_need_anti("b.example.com"));
        // Case-insensitive and port/scheme tolerant.
        assert!(!config.is_need_anti("B.Example.com:443"));
        // An unrelated domain is blocked.
        assert!(config.is_need_anti("evil.example.com"));
    }

    #[test]
    fn disabled_or_empty_never_blocks() {
        let disabled = parse(
            r#"
            [AntiReverseProxy]
            enable = false
            host = ["a.example.com"]
            "#,
        );
        assert!(!disabled.is_need_anti("evil.example.com"));

        let empty = parse(
            r#"
            [AntiReverseProxy]
            enable = true
            host = []
            "#,
        );
        assert!(!empty.is_need_anti("evil.example.com"));
    }

    #[test]
    fn serializes_back_as_list() {
        let config = parse(
            r#"
            [AntiReverseProxy]
            enable = true
            host = "a.example.com"
            "#,
        );

        let rendered = toml::to_string(&config)
            .expect("anti reverse proxy config should serialize");
        assert!(rendered.contains("host = [\"a.example.com\"]"));
    }
}
