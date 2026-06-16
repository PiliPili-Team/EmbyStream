use std::{collections::HashSet, ops::Deref as DerefTrait, sync::Arc};

use dashmap::DashMap;
use tokio::sync::{Mutex as TokioMutex, OnceCell, RwLock as TokioRwLock};

use crate::{
    INIT_LOGGER_DOMAIN,
    cache::{GeneralCache, RateLimiterCache},
    client::{ClientBuilder, EmbyClient, GoogleDriveClient, OpenListClient},
    config::core::Config,
    core::backend::{constants::DISK_BACKEND_TYPE, upstream_proxy, webdav},
    info_log,
    oauthutil::OAuthToken,
    util::path_rewriter::PathRewriter,
};

// These constants define the user agent substrings for clients that require
// a workaround for missing Range headers.
const PROBLEMATIC_CLIENTS: &[&str] =
    &["yamby", "hills", "embytolocalplayer", "Emby/"];
const GOOGLE_DRIVE_FILE_ID_CACHE_TTL_SECS: u64 = 20 * 60;

pub struct AppState {
    pub(crate) config: TokioRwLock<Config>,
    frontend_path_rewrite_cache: OnceCell<Vec<PathRewriter>>,
    problematic_clients_cache: OnceCell<Vec<String>>,
    encrypt_cache: OnceCell<GeneralCache>,
    decrypt_cache: OnceCell<GeneralCache>,
    playback_info_cache: OnceCell<GeneralCache>,
    strm_file_cache: OnceCell<GeneralCache>,
    open_list_cache: OnceCell<GeneralCache>,
    local_metadata_cache: OnceCell<GeneralCache>,
    google_drive_file_id_cache: OnceCell<GeneralCache>,
    emby_client: OnceCell<Arc<EmbyClient>>,
    google_drive_client: OnceCell<Arc<GoogleDriveClient>>,
    open_list_client: OnceCell<Arc<OpenListClient>>,
    rate_limiter_cache: OnceCell<DashMap<String, RateLimiterCache>>,
    pub(crate) open_list_request_locks: DashMap<String, Arc<TokioMutex<()>>>,
    pub(crate) playback_info_request_locks:
        DashMap<String, Arc<TokioMutex<()>>>,
    pub(crate) strm_request_locks: DashMap<String, Arc<TokioMutex<()>>>,
    pub(crate) local_metadata_request_locks:
        DashMap<String, Arc<TokioMutex<()>>>,
    pub(crate) google_drive_file_id_request_locks:
        DashMap<String, Arc<TokioMutex<()>>>,
    pub(crate) google_drive_token_cache: DashMap<String, OAuthToken>,
    pub(crate) google_drive_refresh_locks: DashMap<String, Arc<TokioMutex<()>>>,
    pub(crate) google_drive_refresh_backoff_until:
        DashMap<String, chrono::DateTime<chrono::Utc>>,
    pub(crate) webdav_auth_cache: DashMap<String, String>,
    pub(crate) webdav_auth_probe_locks: DashMap<String, Arc<TokioMutex<()>>>,
}

impl AppState {
    pub async fn new(config: Config) -> Self {
        Self {
            config: TokioRwLock::new(config),
            frontend_path_rewrite_cache: OnceCell::new(),
            problematic_clients_cache: OnceCell::new(),
            encrypt_cache: OnceCell::new(),
            decrypt_cache: OnceCell::new(),
            playback_info_cache: OnceCell::new(),
            strm_file_cache: OnceCell::new(),
            open_list_cache: OnceCell::new(),
            local_metadata_cache: OnceCell::new(),
            google_drive_file_id_cache: OnceCell::new(),
            emby_client: OnceCell::new(),
            google_drive_client: OnceCell::new(),
            open_list_client: OnceCell::new(),
            rate_limiter_cache: OnceCell::new(),
            open_list_request_locks: DashMap::new(),
            playback_info_request_locks: DashMap::new(),
            strm_request_locks: DashMap::new(),
            local_metadata_request_locks: DashMap::new(),
            google_drive_file_id_request_locks: DashMap::new(),
            google_drive_token_cache: DashMap::new(),
            google_drive_refresh_locks: DashMap::new(),
            google_drive_refresh_backoff_until: DashMap::new(),
            webdav_auth_cache: DashMap::new(),
            webdav_auth_probe_locks: DashMap::new(),
        }
    }

    pub async fn get_config(&self) -> impl DerefTrait<Target = Config> + '_ {
        self.config.read().await
    }

    pub async fn get_cache_settings(&self) -> (u64, u64) {
        let config = self.get_config().await;
        match config.general.memory_mode.as_str() {
            "low" => (256, 60 * 60 * 4),
            "high" => (2048, 60 * 60 * 12),
            _ => (512, 60 * 60 * 8),
        }
    }

    pub async fn get_frontend_path_rewrite_cache(&self) -> &Vec<PathRewriter> {
        let config = self.get_config().await;
        self.frontend_path_rewrite_cache
            .get_or_init(|| async move {
                let frontend_config = match &config.frontend {
                    Some(config) => config,
                    None => return vec![],
                };
                frontend_config
                    .clone()
                    .path_rewrites
                    .into_iter()
                    .map(|path_rewrite| {
                        PathRewriter::new(
                            path_rewrite.enable,
                            &path_rewrite.pattern,
                            &path_rewrite.replacement,
                        )
                    })
                    .collect()
            })
            .await
    }

    pub async fn get_problematic_clients(&self) -> &Vec<String> {
        let config = self.get_config().await;
        self.problematic_clients_cache
            .get_or_init(|| async move {
                let mut clients: HashSet<String> = PROBLEMATIC_CLIENTS
                    .iter()
                    .map(|s| s.to_lowercase())
                    .collect();

                if let Some(backend_config) = config.backend.as_ref() {
                    clients.extend(
                        backend_config
                            .problematic_clients
                            .iter()
                            .map(|s| s.to_lowercase()),
                    );
                }

                clients.into_iter().filter(|s| !s.is_empty()).collect()
            })
            .await
    }

    pub async fn get_encrypt_cache(&self) -> &GeneralCache {
        let (capacity, ttl) = self.get_cache_settings().await;
        self.encrypt_cache
            .get_or_init(|| async move { GeneralCache::new(capacity, ttl) })
            .await
    }

    pub async fn get_decrypt_cache(&self) -> &GeneralCache {
        let (capacity, ttl) = self.get_cache_settings().await;
        self.decrypt_cache
            .get_or_init(|| async move { GeneralCache::new(capacity, ttl) })
            .await
    }

    pub async fn get_strm_file_cache(&self) -> &GeneralCache {
        let (capacity, ttl) = self.get_cache_settings().await;
        self.strm_file_cache
            .get_or_init(|| async move { GeneralCache::new(capacity, ttl) })
            .await
    }

    pub async fn get_playback_info_cache(&self) -> &GeneralCache {
        let (capacity, ttl) = self.get_cache_settings().await;
        self.playback_info_cache
            .get_or_init(|| async move { GeneralCache::new(capacity, ttl) })
            .await
    }

    pub async fn get_open_list_cache(&self) -> &GeneralCache {
        let (capacity, ttl) = self.get_cache_settings().await;
        self.open_list_cache
            .get_or_init(|| async move { GeneralCache::new(capacity, ttl) })
            .await
    }

    pub async fn get_local_metadata_cache(&self) -> &GeneralCache {
        let (capacity, _) = self.get_cache_settings().await;
        self.local_metadata_cache
            .get_or_init(
                || async move { GeneralCache::new(capacity, 60 * 60 * 2) },
            )
            .await
    }

    pub async fn get_google_drive_file_id_cache(&self) -> &GeneralCache {
        self.google_drive_file_id_cache
            .get_or_init(|| async move {
                GeneralCache::new(4096, GOOGLE_DRIVE_FILE_ID_CACHE_TTL_SECS)
            })
            .await
    }

    pub async fn get_emby_client(&self) -> &Arc<EmbyClient> {
        self.emby_client
            .get_or_init(|| async move {
                Arc::new(ClientBuilder::<EmbyClient>::new().build())
            })
            .await
    }

    pub async fn get_open_list_client(&self) -> &Arc<OpenListClient> {
        self.open_list_client
            .get_or_init(|| async move {
                Arc::new(ClientBuilder::<OpenListClient>::new().build())
            })
            .await
    }

    pub async fn get_google_drive_client(&self) -> &Arc<GoogleDriveClient> {
        self.google_drive_client
            .get_or_init(|| async move {
                Arc::new(ClientBuilder::<GoogleDriveClient>::new().build())
            })
            .await
    }

    #[cfg(test)]
    pub(crate) fn set_google_drive_client_for_test(
        &self,
        client: Arc<GoogleDriveClient>,
    ) {
        let _ = self.google_drive_client.set(client);
    }

    pub async fn get_rate_limiter_cache(
        &self,
        node_uuid: &str,
    ) -> Option<RateLimiterCache> {
        let cache_map = self
            .rate_limiter_cache
            .get_or_init(|| async move {
                let config = self.get_config().await;
                let (capacity, ttl) = self.get_cache_settings().await;
                let map = DashMap::new();

                // Per-client byte limiting is only applied in `LocalStreamer` (Disk → local file).
                // WebDAV / OpenList / DirectLink / StreamRelay proxy paths do not use this cache.
                for node in &config.backend_nodes {
                    if !node
                        .backend_type
                        .eq_ignore_ascii_case(DISK_BACKEND_TYPE)
                    {
                        continue;
                    }
                    let cache = RateLimiterCache::new(
                        capacity * 2,
                        ttl,
                        node.client_speed_limit_kbs,
                        node.client_burst_speed_kbs,
                    );
                    cache.start_refill_task();
                    map.insert(node.uuid.clone(), cache);
                }

                map
            })
            .await;

        cache_map.get(node_uuid).map(|r| r.value().clone())
    }

    pub async fn init_rate_limiters(&self) {
        self.get_rate_limiter_cache("").await;
    }

    pub(crate) fn request_lock(
        locks: &DashMap<String, Arc<TokioMutex<()>>>,
        cache_key: &str,
    ) -> Arc<TokioMutex<()>> {
        locks
            .entry(cache_key.to_string())
            .or_insert_with(|| Arc::new(TokioMutex::new(())))
            .clone()
    }

    pub(crate) fn cleanup_request_lock(
        locks: &DashMap<String, Arc<TokioMutex<()>>>,
        cache_key: &str,
        lock: &Arc<TokioMutex<()>>,
    ) {
        let Some(entry) = locks.get(cache_key) else {
            return;
        };

        let should_remove = Arc::ptr_eq(entry.value(), lock)
            && Arc::strong_count(entry.value()) == 2;
        drop(entry);

        if should_remove {
            let _ = locks.remove(cache_key);
        }
    }

    /// Warms up WebDAV connections during startup to reduce first-request latency.
    /// Pre-establishes TCP/TLS connections to all configured WebDAV nodes.
    pub async fn warmup_webdav_connections(&self) {
        let config = self.get_config().await;
        let webdav_nodes: Vec<_> = config
            .backend_nodes
            .iter()
            .filter(|node| {
                node.backend_type.eq_ignore_ascii_case(webdav::BACKEND_TYPE)
            })
            .collect();

        if webdav_nodes.is_empty() {
            return;
        }

        info_log!(
            INIT_LOGGER_DOMAIN,
            "Warming up {} WebDAV connections...",
            webdav_nodes.len()
        );

        let mut tasks = Vec::new();
        for node in webdav_nodes {
            if let Ok(uri) = node.base_url.parse() {
                let task = tokio::spawn(async move {
                    let _ = upstream_proxy::warmup_connection(uri).await;
                });
                tasks.push(task);
            }
        }

        for task in tasks {
            let _ = task.await;
        }

        info_log!(INIT_LOGGER_DOMAIN, "WebDAV connection warmup completed");
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use super::AppState;
    use dashmap::DashMap;
    use tokio::sync::Mutex as TokioMutex;

    use crate::config::core::{finish_raw_config, parse_raw_config_str};

    const MIN_FRONTEND_CONFIG: &str = r#"
[Log]
level = "info"
prefix = ""
root_path = "./logs"

[General]
memory_mode = "middle"
stream_mode = "frontend"
encipher_key = "1234567890123456"
encipher_iv = "1234567890123456"

[Emby]
url = "http://127.0.0.1"
port = "8096"
token = "tok"

[UserAgent]
mode = "allow"
allow_ua = []
deny_ua = []

[Fallback]

[Frontend]
listen_port = 60001

[Frontend.AntiReverseProxy]
enable = false
host = ""
"#;

    async fn test_state() -> AppState {
        let raw = parse_raw_config_str(MIN_FRONTEND_CONFIG).expect("parse");
        let config =
            finish_raw_config(PathBuf::from("test.toml"), raw).expect("finish");
        AppState::new(config).await
    }

    #[tokio::test]
    async fn cleanup_request_lock_removes_unshared_lock() {
        let locks = DashMap::<String, Arc<TokioMutex<()>>>::new();
        let lock = AppState::request_lock(&locks, "key");

        {
            let _guard = lock.lock().await;
        }

        AppState::cleanup_request_lock(&locks, "key", &lock);

        assert!(locks.is_empty());
    }

    #[test]
    fn cleanup_request_lock_keeps_shared_lock() {
        let locks = DashMap::<String, Arc<TokioMutex<()>>>::new();
        let lock = AppState::request_lock(&locks, "key");
        let _other_ref = lock.clone();

        AppState::cleanup_request_lock(&locks, "key", &lock);

        assert_eq!(locks.len(), 1);
    }

    #[tokio::test]
    async fn get_emby_client_reuses_single_instance() {
        let state = test_state().await;

        let client1 = state.get_emby_client().await.clone();
        let client2 = state.get_emby_client().await.clone();

        assert!(Arc::ptr_eq(&client1, &client2));
    }

    #[tokio::test]
    async fn get_open_list_client_reuses_single_instance() {
        let state = test_state().await;

        let client1 = state.get_open_list_client().await.clone();
        let client2 = state.get_open_list_client().await.clone();

        assert!(Arc::ptr_eq(&client1, &client2));
    }
}
