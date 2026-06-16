use std::{sync::Arc, time::Instant};

use serde_json::Value as JsonValue;

use crate::{
    AppState, PLAYBACK_INFO_LOGGER_DOMAIN, api::PlaybackInfo,
    cache::GeneralCache, core::frontend::types::InfuseAuthorization, debug_log,
    info_log, network::HttpMethod, util::StringUtil, warn_log,
};

const SLOW_PLAYBACK_INFO_FETCH_THRESHOLD_MS: u128 = 500;
const PLAYBACK_INFO_RESPONSE_KEY_PREFIX: &str = "playback:info:response";
const PLAYBACK_INFO_ITEM_INDEX_KEY_PREFIX: &str = "playback:info:item_index";
const PLAYBACK_INFO_ITEM_ID_SEGMENT: &str = "item_id";
const PLAYBACK_INFO_ITEMS_SEGMENT: &str = "Items";
const PLAYBACK_INFO_PATH_SEGMENT: &str = "PlaybackInfo";
const PLAYBACK_INFO_IGNORED_QUERY_KEYS: &[&str] = &["api_key", "x-emby-token"];
const CONTENT_TYPE_JSON: &str = "application/json";
const CONTENT_TYPE_FORM_URLENCODED: &str = "application/x-www-form-urlencoded";

/// A request to fetch PlaybackInfo from Emby.
///
/// EmbyStream issues PlaybackInfo fetches via POST (mirroring the client), so
/// `media_source_id` is optional — Emby returns every media source for the item
/// when it is empty, which is exactly what stream-path lookup needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybackInfoRequest {
    pub item_id: String,
    pub media_source_id: String,
    pub method: HttpMethod,
    pub body: Option<Vec<u8>>,
    pub content_type: Option<String>,
}

impl PlaybackInfoRequest {
    pub fn new(
        item_id: impl Into<String>,
        media_source_id: impl Into<String>,
        method: HttpMethod,
        body: Option<Vec<u8>>,
        content_type: Option<String>,
    ) -> Self {
        Self {
            item_id: item_id.into(),
            media_source_id: media_source_id.into(),
            method,
            body,
            content_type,
        }
    }

    /// Extracts the item id from an Emby PlaybackInfo path such as
    /// `/emby/Items/{item_id}/PlaybackInfo` (the `emby/` prefix is optional).
    pub fn item_id_from_path(path: &str) -> Option<String> {
        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();

        segments
            .windows(3)
            .find(|window| {
                window.first().is_some_and(|segment| {
                    segment.eq_ignore_ascii_case(PLAYBACK_INFO_ITEMS_SEGMENT)
                }) && window.get(2).is_some_and(|segment| {
                    segment.eq_ignore_ascii_case(PLAYBACK_INFO_PATH_SEGMENT)
                })
            })
            .and_then(|window| window.get(1))
            .map(|segment| (*segment).to_string())
    }
}

#[derive(Debug)]
pub enum PlaybackInfoServiceError {
    InvalidItemId,
    EmptyApiToken,
    Upstream(anyhow::Error),
}

impl std::fmt::Display for PlaybackInfoServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidItemId => write!(f, "invalid playback info item id"),
            Self::EmptyApiToken => write!(f, "empty playback info api token"),
            Self::Upstream(error) => {
                write!(f, "playback info upstream: {error}")
            }
        }
    }
}

impl std::error::Error for PlaybackInfoServiceError {}

#[derive(Clone)]
pub struct PlaybackInfoService {
    state: Arc<AppState>,
}

impl PlaybackInfoService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    /// Builds the response cache key for a PlaybackInfo request.
    ///
    /// The key is a normalized hash over the item id plus every query key/value
    /// pair (keys lowercased, pairs sorted, auth credentials excluded). Distinct
    /// request parameters make Emby return slightly different responses, so they
    /// must map to distinct cache entries; requests that differ only in
    /// parameter order share a single entry.
    pub fn response_cache_key(item_id: &str, query: Option<&str>) -> String {
        let normalized = format!(
            "{}|{}",
            item_id.trim().to_ascii_lowercase(),
            Self::canonical_query(query)
        );
        let digest = StringUtil::hash_hex(&normalized);
        format!("{PLAYBACK_INFO_RESPONSE_KEY_PREFIX}:hash:{digest}")
    }

    fn canonical_query(query: Option<&str>) -> String {
        let Some(query) = query.map(str::trim).filter(|q| !q.is_empty()) else {
            return String::new();
        };

        let mut pairs: Vec<(String, String)> =
            form_urlencoded::parse(query.as_bytes())
                .filter_map(|(key, value)| {
                    let key = key.to_ascii_lowercase();
                    if PLAYBACK_INFO_IGNORED_QUERY_KEYS.contains(&key.as_str())
                    {
                        return None;
                    }
                    Some((key, value.into_owned()))
                })
                .collect();
        pairs.sort_by(|(left_key, left_value), (right_key, right_value)| {
            left_key
                .cmp(right_key)
                .then_with(|| left_value.cmp(right_value))
        });

        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(
                pairs
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            )
            .finish()
    }

    fn item_index_cache_key(
        item_id: &str,
    ) -> Result<String, PlaybackInfoServiceError> {
        let item_id = item_id.trim();
        if item_id.is_empty() {
            return Err(PlaybackInfoServiceError::InvalidItemId);
        }
        Ok(format!(
            "{PLAYBACK_INFO_ITEM_INDEX_KEY_PREFIX}:{PLAYBACK_INFO_ITEM_ID_SEGMENT}:{}",
            item_id.to_ascii_lowercase()
        ))
    }

    fn store_item_index_into(
        cache: &GeneralCache,
        item_id: &str,
        playback_info: &PlaybackInfo,
    ) {
        let Ok(index_key) = Self::item_index_cache_key(item_id) else {
            return;
        };
        cache.insert(index_key.clone(), playback_info.clone());
        info_log!(
            PLAYBACK_INFO_LOGGER_DOMAIN,
            "playback_info_item_index_store key={} media_sources={}",
            index_key,
            playback_info.media_sources.len()
        );
    }

    /// Returns the cached PlaybackInfo response JSON for a key, if present.
    pub async fn cached_response(&self, cache_key: &str) -> Option<String> {
        let cache = self.state.get_playback_info_cache().await;
        let cached = cache.get::<String>(cache_key);
        if cached.is_some() {
            info_log!(
                PLAYBACK_INFO_LOGGER_DOMAIN,
                "playback_info_response_cache_hit key={}",
                cache_key
            );
        }
        cached
    }

    /// Caches a PlaybackInfo response and indexes its media sources by item id.
    ///
    /// The raw upstream JSON is stored verbatim under `cache_key` for byte-
    /// faithful replay to clients. The parsed media sources are indexed by item
    /// id so stream requests — which omit `MediaSourceId` — can derive a path
    /// without a redundant Emby round-trip.
    pub async fn store_response_and_index(
        &self,
        cache_key: &str,
        item_id: &str,
        body: &[u8],
    ) {
        let cache = self.state.get_playback_info_cache().await;

        match std::str::from_utf8(body) {
            Ok(text) => {
                cache.insert(cache_key.to_string(), text.to_string());
                info_log!(
                    PLAYBACK_INFO_LOGGER_DOMAIN,
                    "playback_info_response_cache_store key={} bytes={}",
                    cache_key,
                    body.len()
                );
            }
            Err(error) => {
                warn_log!(
                    PLAYBACK_INFO_LOGGER_DOMAIN,
                    "playback_info_response_non_utf8 key={} error={}",
                    cache_key,
                    error
                );
                return;
            }
        }

        match serde_json::from_slice::<PlaybackInfo>(body) {
            Ok(playback_info) => {
                Self::store_item_index_into(cache, item_id, &playback_info)
            }
            Err(error) => {
                warn_log!(
                    PLAYBACK_INFO_LOGGER_DOMAIN,
                    "playback_info_index_parse_failed item_id={} error={}",
                    item_id,
                    error
                );
            }
        }
    }

    /// Loads the full PlaybackInfo for an item, keyed purely by item id.
    ///
    /// Prefers the item index populated by earlier PlaybackInfo traffic and
    /// falls back to a single Emby POST on a miss. The POST returns every media
    /// source for the item, so it works even when `media_source_id` is absent —
    /// exactly the case for stream requests that omit it.
    pub async fn get_or_fetch_by_item_id(
        &self,
        item_id: &str,
        media_source_id: Option<&str>,
        api_token: Option<&str>,
    ) -> Result<PlaybackInfo, PlaybackInfoServiceError> {
        let index_key = Self::item_index_cache_key(item_id)?;
        let cache = self.state.get_playback_info_cache().await;

        if let Some(cached) = cache.get::<PlaybackInfo>(&index_key) {
            info_log!(
                PLAYBACK_INFO_LOGGER_DOMAIN,
                "playback_info_item_index_hit key={}",
                index_key
            );
            return Ok(cached);
        }

        let lock = AppState::request_lock(
            &self.state.playback_info_request_locks,
            &index_key,
        );

        let result = {
            let wait_start = Instant::now();
            let _guard = lock.lock().await;
            let wait_ms = wait_start.elapsed().as_millis();

            if let Some(cached) = cache.get::<PlaybackInfo>(&index_key) {
                info_log!(
                    PLAYBACK_INFO_LOGGER_DOMAIN,
                    "playback_info_item_index_inflight_wait_hit key={} \
                     lock_wait_ms={}",
                    index_key,
                    wait_ms
                );
                Ok(cached)
            } else {
                let token = api_token
                    .map(str::trim)
                    .filter(|token| !token.is_empty())
                    .ok_or(PlaybackInfoServiceError::EmptyApiToken)?;

                let request = PlaybackInfoRequest::new(
                    item_id,
                    media_source_id.unwrap_or_default(),
                    HttpMethod::Post,
                    None,
                    None,
                );

                let fetch_start = Instant::now();
                let playback_info =
                    self.fetch_from_emby(&request, token).await?;
                let fetch_ms = fetch_start.elapsed().as_millis();

                if fetch_ms >= SLOW_PLAYBACK_INFO_FETCH_THRESHOLD_MS {
                    warn_log!(
                        PLAYBACK_INFO_LOGGER_DOMAIN,
                        "playback_info_item_index_fetch_slow item_id={} \
                         elapsed_ms={}",
                        item_id,
                        fetch_ms
                    );
                } else {
                    debug_log!(
                        PLAYBACK_INFO_LOGGER_DOMAIN,
                        "playback_info_item_index_fetch_complete item_id={} \
                         elapsed_ms={}",
                        item_id,
                        fetch_ms
                    );
                }

                Self::store_item_index_into(cache, item_id, &playback_info);
                Ok(playback_info)
            }
        };

        AppState::cleanup_request_lock(
            &self.state.playback_info_request_locks,
            &index_key,
            &lock,
        );

        result
    }

    pub fn api_token_from_headers_and_query(
        headers: &hyper::HeaderMap,
        query: Option<&str>,
    ) -> Option<String> {
        query
            .and_then(Self::api_token_from_query)
            .or_else(|| Self::api_token_from_headers(headers))
    }

    pub fn api_token_from_headers_query_and_body(
        headers: &hyper::HeaderMap,
        query: Option<&str>,
        body: Option<&[u8]>,
        content_type: Option<&str>,
    ) -> Option<String> {
        Self::api_token_from_headers_and_query(headers, query)
            .or_else(|| Self::api_token_from_body(body, content_type))
    }

    async fn fetch_from_emby(
        &self,
        request: &PlaybackInfoRequest,
        api_token: &str,
    ) -> Result<PlaybackInfo, PlaybackInfoServiceError> {
        let config = self.state.get_config().await;
        let emby_server_url = config.emby.get_uri().to_string();
        let emby_client = self.state.get_emby_client().await.clone();

        emby_client
            .playback_info(emby_server_url, api_token.to_string(), request)
            .await
            .map_err(PlaybackInfoServiceError::Upstream)
    }

    fn api_token_from_query(query: &str) -> Option<String> {
        form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| Self::is_api_token_key(key))
            .map(|(_, value)| value.into_owned())
    }

    fn api_token_from_headers(headers: &hyper::HeaderMap) -> Option<String> {
        headers
            .get("X-Emby-Token")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .or_else(|| {
                headers
                    .get("x-emby-authorization")
                    .and_then(|value| value.to_str().ok())
                    .and_then(InfuseAuthorization::from_header_str)
                    .and_then(|auth| auth.get("MediaBrowser Token"))
            })
    }

    fn api_token_from_body(
        body: Option<&[u8]>,
        content_type: Option<&str>,
    ) -> Option<String> {
        let body = body?;
        let trimmed = std::str::from_utf8(body).ok()?.trim();
        if trimmed.is_empty() {
            return None;
        }

        match Self::normalized_body_content_type(content_type) {
            Some(CONTENT_TYPE_JSON) => Self::api_token_from_json(trimmed),
            Some(CONTENT_TYPE_FORM_URLENCODED) => {
                Self::api_token_from_form(trimmed)
            }
            Some("text/plain") | None => Self::api_token_from_json(trimmed)
                .or_else(|| Self::api_token_from_form(trimmed)),
            _ => None,
        }
    }

    fn api_token_from_json(body: &str) -> Option<String> {
        let value = serde_json::from_str::<JsonValue>(body).ok()?;
        match value {
            JsonValue::Object(map) => {
                map.into_iter().find_map(|(key, value)| {
                    if !Self::is_api_token_key(&key) {
                        return None;
                    }
                    value
                        .as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                })
            }
            _ => None,
        }
    }

    fn api_token_from_form(body: &str) -> Option<String> {
        form_urlencoded::parse(body.as_bytes())
            .find(|(key, _)| Self::is_api_token_key(key))
            .map(|(_, value)| value.into_owned())
    }

    fn is_api_token_key(key: &str) -> bool {
        key.eq_ignore_ascii_case("api_key")
            || key.eq_ignore_ascii_case("X-Emby-Token")
            || key.eq_ignore_ascii_case("Token")
            || key.eq_ignore_ascii_case("MediaBrowser Token")
    }

    fn normalized_body_content_type(
        content_type: Option<&str>,
    ) -> Option<&str> {
        content_type
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
    }
}

#[cfg(test)]
mod tests {
    use super::{CONTENT_TYPE_JSON, PlaybackInfoRequest, PlaybackInfoService};

    #[test]
    fn item_id_from_path_extracts_id_with_emby_prefix() {
        assert_eq!(
            PlaybackInfoRequest::item_id_from_path(
                "/emby/Items/249971/PlaybackInfo"
            ),
            Some("249971".to_string())
        );
    }

    #[test]
    fn item_id_from_path_extracts_id_without_emby_prefix() {
        assert_eq!(
            PlaybackInfoRequest::item_id_from_path(
                "/Items/249971/PlaybackInfo"
            ),
            Some("249971".to_string())
        );
    }

    #[test]
    fn item_id_from_path_rejects_non_playback_info_path() {
        assert!(
            PlaybackInfoRequest::item_id_from_path("/emby/Items/249971")
                .is_none()
        );
    }

    #[test]
    fn response_cache_key_is_query_order_independent() {
        let key1 = PlaybackInfoService::response_cache_key(
            "23090",
            Some("IsPlayback=false&reqformat=json&StartTimeTicks=0"),
        );
        let key2 = PlaybackInfoService::response_cache_key(
            "23090",
            Some("StartTimeTicks=0&reqformat=json&IsPlayback=false"),
        );

        assert_eq!(key1, key2);
        assert!(key1.starts_with("playback:info:response:hash:"));
    }

    #[test]
    fn response_cache_key_is_item_id_case_insensitive() {
        let lower = PlaybackInfoService::response_cache_key("abc", Some("a=1"));
        let upper = PlaybackInfoService::response_cache_key("ABC", Some("a=1"));

        assert_eq!(lower, upper);
    }

    #[test]
    fn response_cache_key_differs_on_query_value() {
        let low_bitrate = PlaybackInfoService::response_cache_key(
            "23090",
            Some("MaxStreamingBitrate=500000"),
        );
        let high_bitrate = PlaybackInfoService::response_cache_key(
            "23090",
            Some("MaxStreamingBitrate=999999"),
        );

        assert_ne!(low_bitrate, high_bitrate);
    }

    #[test]
    fn response_cache_key_differs_on_item_id() {
        let item_a =
            PlaybackInfoService::response_cache_key("23090", Some("a=1"));
        let item_b =
            PlaybackInfoService::response_cache_key("73426", Some("a=1"));

        assert_ne!(item_a, item_b);
    }

    #[test]
    fn response_cache_key_ignores_auth_credentials() {
        let with_token = PlaybackInfoService::response_cache_key(
            "23090",
            Some("reqformat=json&api_key=secret&X-Emby-Token=tok"),
        );
        let without_token = PlaybackInfoService::response_cache_key(
            "23090",
            Some("reqformat=json"),
        );

        assert_eq!(with_token, without_token);
    }

    #[test]
    fn item_index_cache_key_is_media_source_independent() {
        let key = PlaybackInfoService::item_index_cache_key("Item-42");

        assert_eq!(
            key.unwrap_or_default(),
            "playback:info:item_index:item_id:item-42"
        );
    }

    #[test]
    fn item_index_cache_key_rejects_blank_item_id() {
        assert!(PlaybackInfoService::item_index_cache_key("   ").is_err());
    }

    #[test]
    fn playback_info_service_parses_api_token_from_unquoted_emby_header() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            "x-emby-authorization",
            hyper::header::HeaderValue::from_static(
                "Emby UserId=user1,Client=Yamby,Device=Phone,DeviceId=device1,Version=1.0,Token=abc123",
            ),
        );

        assert_eq!(
            PlaybackInfoService::api_token_from_headers_and_query(
                &headers, None
            ),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn playback_info_service_parses_api_token_from_json_body() {
        let headers = hyper::HeaderMap::new();

        assert_eq!(
            PlaybackInfoService::api_token_from_headers_query_and_body(
                &headers,
                None,
                Some(br#"{"Token":"abc123"}"#),
                Some(CONTENT_TYPE_JSON),
            ),
            Some("abc123".to_string())
        );
    }
}
