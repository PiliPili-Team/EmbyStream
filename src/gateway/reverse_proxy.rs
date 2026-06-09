use std::{fmt::Debug, sync::Arc, time::Instant};

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::{
    Method, Response, StatusCode,
    body::Incoming,
    header::{self, HeaderName, HeaderValue},
};
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::{
    cacheable_routes::BodyKeyStrategy,
    cacheable_routes::build_semantic_cache_key,
    cacheable_routes::find_cacheable_route,
    chain::{Middleware, Next},
    context::Context,
    response::{BoxBodyType, ResponseBuilder},
};
use crate::{
    API_CACHE_LOGGER_DOMAIN, AppState, REVERSE_PROXY_LOGGER_DOMAIN,
    cache::GeneralCache,
    client::{
        PlaybackInfoRequest, PlaybackInfoService, PlaybackInfoServiceError,
    },
    debug_log, error_log,
    util::StringUtil,
    warn_log,
};
use tokio::sync::Mutex as TokioMutex;

const ROOT_PATH: &str = "/";
const WEB_INDEX_REDIRECT: &str = "/web/index.html";
const MAX_CACHEABLE_BODY_BYTES: usize = 64 * 1024;
const PLAYBACK_INFO_PATH_SEGMENT: &str = "PlaybackInfo";

#[derive(Clone, Debug)]
struct CachedApiResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    // `GeneralCache` TTL is the upper retention bound for API entries.
    // Route freshness is enforced separately here so different routes can
    // still have their own shorter logical cache lifetime.
    stored_at: Instant,
    route_ttl_seconds: u64,
}

impl CachedApiResponse {
    fn is_expired(&self) -> bool {
        self.stored_at.elapsed().as_secs() > self.route_ttl_seconds
    }

    fn to_response(&self) -> Response<BoxBodyType> {
        let headers: Vec<(HeaderName, HeaderValue)> = self
            .headers
            .iter()
            .filter_map(|(name, value)| {
                let header_name = name.parse::<HeaderName>().ok()?;
                let header_value = HeaderValue::from_str(value).ok()?;
                Some((header_name, header_value))
            })
            .collect();

        let status =
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::OK);

        ResponseBuilder::with_bytes(
            status,
            headers,
            Bytes::from(self.body.clone()),
        )
    }
}

#[derive(Clone)]
pub struct ReverseProxyMiddleware {
    emby_base_url: String,
    http_client: reqwest::Client,
    api_cache: GeneralCache,
    state: Arc<AppState>,
    playback_info_service: PlaybackInfoService,
}

impl ReverseProxyMiddleware {
    pub fn new(
        emby_base_url: String,
        api_cache: GeneralCache,
        state: std::sync::Arc<AppState>,
    ) -> Self {
        let http_client = match reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                warn_log!(
                    REVERSE_PROXY_LOGGER_DOMAIN,
                    "reverse_proxy_client_build_failed_using_fallback error={}",
                    error
                );
                reqwest::Client::new()
            }
        };

        Self {
            emby_base_url,
            http_client,
            api_cache,
            state: state.clone(),
            playback_info_service: PlaybackInfoService::new(state),
        }
    }

    async fn read_body(body: Option<Incoming>) -> Option<Bytes> {
        let incoming = body?;
        match incoming.collect().await {
            Ok(collected) => Some(collected.to_bytes()),
            Err(e) => {
                warn_log!(
                    REVERSE_PROXY_LOGGER_DOMAIN,
                    "Failed to read request body: {:?}",
                    e
                );
                None
            }
        }
    }

    fn build_cache_key(
        ctx: &Context,
        route: &super::cacheable_routes::CompiledCacheableRoute,
        body_bytes: Option<&Bytes>,
    ) -> Option<String> {
        let semantic_key = build_semantic_cache_key(
            route,
            ctx.method.as_str(),
            &ctx.path,
            ctx.uri.query(),
        );

        let Some(bytes) = body_bytes.filter(|bytes| !bytes.is_empty()) else {
            return Some(semantic_key);
        };

        if matches!(route.body_key_strategy, BodyKeyStrategy::Ignore) {
            return Some(semantic_key);
        }

        let body_hash =
            Self::body_hash(route.body_key_strategy, &ctx.headers, bytes)?;

        Some(format!("{semantic_key}:body_hash:{body_hash}"))
    }

    fn body_hash(
        strategy: BodyKeyStrategy,
        headers: &hyper::HeaderMap,
        bytes: &Bytes,
    ) -> Option<String> {
        if bytes.len() > MAX_CACHEABLE_BODY_BYTES {
            return None;
        }

        match strategy {
            BodyKeyStrategy::Ignore => None,
            BodyKeyStrategy::RawHash => Some(Self::raw_body_hash(bytes)),
            BodyKeyStrategy::JsonCanonical => Some(Self::json_body_hash(bytes)),
            BodyKeyStrategy::FormUrlEncodedCanonical => {
                Some(Self::form_body_hash(bytes))
            }
            BodyKeyStrategy::AutoContentType => {
                let body_hash = match Self::request_content_type(headers) {
                    Some("application/json") => Self::json_body_hash(bytes),
                    Some("application/x-www-form-urlencoded") => {
                        Self::form_body_hash(bytes)
                    }
                    _ => return None,
                };
                Some(body_hash)
            }
        }
    }

    fn raw_body_hash(bytes: &Bytes) -> String {
        StringUtil::hash_bytes(bytes.as_ref())
    }

    fn json_body_hash(bytes: &Bytes) -> String {
        let canonical_json = serde_json::from_slice::<JsonValue>(bytes)
            .map(Self::canonicalize_json_value)
            .and_then(|value| serde_json::to_vec(&value))
            .unwrap_or_else(|_| bytes.to_vec());

        StringUtil::hash_bytes(&canonical_json)
    }

    fn form_body_hash(bytes: &Bytes) -> String {
        let body = std::str::from_utf8(bytes.as_ref())
            .unwrap_or_default()
            .trim();

        if body.is_empty() {
            return String::new();
        }

        let mut form_pairs: Vec<(String, String)> =
            form_urlencoded::parse(body.as_bytes())
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
        form_pairs.sort_by(
            |(left_key, left_value), (right_key, right_value)| {
                left_key
                    .to_ascii_lowercase()
                    .cmp(&right_key.to_ascii_lowercase())
                    .then_with(|| left_value.cmp(right_value))
            },
        );

        let normalized_form = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(
                form_pairs
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            )
            .finish();

        StringUtil::hash_hex(&normalized_form)
    }

    fn request_content_type(headers: &hyper::HeaderMap) -> Option<&str> {
        headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
    }

    fn canonicalize_json_value(value: JsonValue) -> JsonValue {
        match value {
            JsonValue::Array(values) => JsonValue::Array(
                values
                    .into_iter()
                    .map(Self::canonicalize_json_value)
                    .collect(),
            ),
            JsonValue::Object(map) => {
                let mut entries: Vec<(String, JsonValue)> = map
                    .into_iter()
                    .map(|(key, value)| {
                        (key, Self::canonicalize_json_value(value))
                    })
                    .collect();
                entries.sort_by(|(left_key, _), (right_key, _)| {
                    left_key.cmp(right_key)
                });

                let canonical_map: JsonMap<String, JsonValue> =
                    entries.into_iter().collect();
                JsonValue::Object(canonical_map)
            }
            other => other,
        }
    }

    fn try_cache_hit(&self, cache_key: &str) -> Option<Response<BoxBodyType>> {
        let cached: CachedApiResponse = self.api_cache.get(cache_key)?;

        if cached.is_expired() {
            self.api_cache.remove(cache_key);
            debug_log!(
                API_CACHE_LOGGER_DOMAIN,
                "[CACHE EXPIRED] key={}",
                cache_key
            );
            return None;
        }

        debug_log!(
            API_CACHE_LOGGER_DOMAIN,
            "[CACHE HIT] key={}{}",
            cache_key,
            Self::cache_key_log_suffix(cache_key)
        );
        Some(cached.to_response())
    }

    fn store_cache(
        &self,
        cache_key: String,
        status: StatusCode,
        headers: &reqwest::header::HeaderMap,
        body: &Bytes,
        ttl_seconds: u64,
    ) {
        let header_pairs: Vec<(String, String)> = headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.as_str().to_owned(), v.to_owned()))
            })
            .collect();

        let cached = CachedApiResponse {
            status: status.as_u16(),
            headers: header_pairs,
            body: body.to_vec(),
            stored_at: Instant::now(),
            route_ttl_seconds: ttl_seconds,
        };

        self.api_cache.insert(cache_key.clone(), cached);
        debug_log!(
            API_CACHE_LOGGER_DOMAIN,
            "[CACHE STORE] key={}, ttl={}s, body_size={}{}",
            cache_key,
            ttl_seconds,
            body.len(),
            Self::cache_key_log_suffix(&cache_key)
        );
    }

    fn should_cache_response(
        status: StatusCode,
        headers: &reqwest::header::HeaderMap,
    ) -> bool {
        status.is_success() && Self::is_json_content_type(headers)
    }

    fn is_json_content_type(headers: &reqwest::header::HeaderMap) -> bool {
        headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(|value| value.trim())
            .is_some_and(|value| value.eq_ignore_ascii_case("application/json"))
    }

    /// Joins the Emby base URL with the incoming `path?query` while keeping
    /// exactly one slash between them.
    ///
    /// `Uri::to_string()` normalizes an authority-only URL such as
    /// `http://host:8096` into `http://host:8096/` (a trailing slash is
    /// always appended). Naively concatenating that with a request target
    /// like `/emby/Users/{id}` would yield `http://host:8096//emby/...`.
    /// The extra empty path segment breaks Emby's route binding for
    /// `Guid`-typed parameters and surfaces as `Unrecognized Guid format.`.
    fn build_target_url(base: &str, path_and_query: &str) -> String {
        let base = base.trim_end_matches('/');
        if path_and_query.starts_with('/') {
            format!("{base}{path_and_query}")
        } else {
            format!("{base}/{path_and_query}")
        }
    }

    async fn proxy_to_emby(
        &self,
        ctx: &Context,
        body_bytes: Option<Bytes>,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let target_url = Self::build_target_url(
            &self.emby_base_url,
            ctx.uri
                .path_and_query()
                .map(|pq| pq.as_str())
                .unwrap_or(ctx.path.as_str()),
        );

        debug_log!(
            REVERSE_PROXY_LOGGER_DOMAIN,
            "Proxying {} {} -> {}",
            ctx.method,
            ctx.path,
            target_url
        );

        let method =
            reqwest::Method::from_bytes(ctx.method.as_str().as_bytes())
                .unwrap_or(reqwest::Method::GET);

        let mut request_builder = self.http_client.request(method, &target_url);

        for (name, value) in ctx.headers.iter() {
            if name == header::HOST || name == header::TRANSFER_ENCODING {
                continue;
            }
            if let Ok(value_str) = value.to_str() {
                request_builder =
                    request_builder.header(name.as_str(), value_str);
            }
        }

        if let Some(bytes) = body_bytes {
            if !bytes.is_empty() {
                request_builder = request_builder.body(bytes);
            }
        }

        request_builder.send().await
    }

    fn build_proxy_response(
        status: StatusCode,
        headers: &reqwest::header::HeaderMap,
        body_bytes: Bytes,
    ) -> Response<BoxBodyType> {
        let response_headers: Vec<(HeaderName, HeaderValue)> = headers
            .iter()
            .filter_map(|(name, value)| {
                if name == header::TRANSFER_ENCODING {
                    return None;
                }
                let hn =
                    HeaderName::from_bytes(name.as_str().as_bytes()).ok()?;
                let hv = HeaderValue::from_bytes(value.as_bytes()).ok()?;
                Some((hn, hv))
            })
            .collect();

        ResponseBuilder::with_bytes(status, response_headers, body_bytes)
    }

    async fn proxy_and_read(
        &self,
        ctx: &Context,
        body_bytes: Option<Bytes>,
    ) -> Option<(StatusCode, reqwest::header::HeaderMap, Bytes)> {
        let emby_response = match self.proxy_to_emby(ctx, body_bytes).await {
            Ok(resp) => resp,
            Err(e) => {
                error_log!(
                    REVERSE_PROXY_LOGGER_DOMAIN,
                    "Failed to proxy to Emby: {:?}",
                    e
                );
                return None;
            }
        };

        let status = StatusCode::from_u16(emby_response.status().as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let resp_headers = emby_response.headers().clone();

        match emby_response.bytes().await {
            Ok(resp_body) => Some((status, resp_headers, resp_body)),
            Err(e) => {
                error_log!(
                    REVERSE_PROXY_LOGGER_DOMAIN,
                    "Failed to read Emby response body: {:?}",
                    e
                );
                None
            }
        }
    }

    async fn lock_api_request(&self, cache_key: &str) -> Arc<TokioMutex<()>> {
        AppState::request_lock(&self.state.api_request_locks, cache_key)
    }

    fn cache_key_log_suffix(cache_key: &str) -> String {
        if let Some(series_id) =
            cache_key.strip_prefix("api:shows_nextup:method:get:series_id:")
        {
            return format!(" route=shows_nextup series_id={series_id}");
        }

        if let Some(show_id) =
            cache_key.strip_prefix("api:shows_episodes:method:get:show_id:")
        {
            return format!(" route=shows_episodes show_id={show_id}");
        }

        if let Some(rest) =
            cache_key.strip_prefix("api:user_item:method:get:user_id:")
        {
            if let Some((user_id, item_id)) = rest.split_once(":item_id:") {
                return format!(
                    " route=user_item user_id={user_id} item_id={item_id}"
                );
            }
        }

        String::new()
    }

    fn playback_info_request(
        &self,
        ctx: &Context,
        body_bytes: Option<&[u8]>,
    ) -> Option<PlaybackInfoRequest> {
        if ctx.method != Method::GET && ctx.method != Method::POST {
            return None;
        }

        let content_type = ctx
            .headers
            .get(hyper::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        PlaybackInfoRequest::from_http_parts(
            &ctx.path,
            ctx.uri.query(),
            &ctx.method,
            body_bytes,
            content_type,
        )
        .ok()
    }

    fn should_passthrough_playback_info_error(
        error: &PlaybackInfoServiceError,
    ) -> bool {
        matches!(
            error,
            PlaybackInfoServiceError::InvalidItemId
                | PlaybackInfoServiceError::InvalidMediaSourceId
                | PlaybackInfoServiceError::UnsupportedMethod
                | PlaybackInfoServiceError::EmptyApiToken
        )
    }

    async fn proxy_playback_info_passthrough(
        &self,
        ctx: &Context,
        body_bytes: Option<Bytes>,
    ) -> Response<BoxBodyType> {
        match self.proxy_and_read(ctx, body_bytes).await {
            Some((status, resp_headers, resp_body)) => {
                Self::build_proxy_response(status, &resp_headers, resp_body)
            }
            None => ResponseBuilder::with_status_code(StatusCode::BAD_GATEWAY),
        }
    }

    async fn handle_playback_info_request(
        &self,
        ctx: &Context,
        body: Option<Incoming>,
    ) -> Response<BoxBodyType> {
        let body_bytes = Self::read_body(body).await;
        let content_type = ctx
            .headers
            .get(hyper::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        let Some(request) =
            self.playback_info_request(ctx, body_bytes.as_deref())
        else {
            warn_log!(
                REVERSE_PROXY_LOGGER_DOMAIN,
                "playback_info_request_parse_failed method={} path={} \
                 fallback=proxy_to_emby",
                ctx.method,
                ctx.path
            );
            return self.proxy_playback_info_passthrough(ctx, body_bytes).await;
        };
        let api_token =
            PlaybackInfoService::api_token_from_headers_query_and_body(
                &ctx.headers,
                ctx.uri.query(),
                body_bytes.as_deref(),
                content_type,
            );

        let playback_info = match self
            .playback_info_service
            .get(&request, api_token.as_deref())
            .await
        {
            Ok(playback_info) => playback_info,
            Err(error) => {
                if Self::should_passthrough_playback_info_error(&error) {
                    warn_log!(
                        REVERSE_PROXY_LOGGER_DOMAIN,
                        "playback_info_request_unhandled_locally method={} \
                         path={} error={} fallback=proxy_to_emby",
                        ctx.method,
                        ctx.path,
                        error
                    );
                    return self
                        .proxy_playback_info_passthrough(ctx, body_bytes)
                        .await;
                }

                let status = StatusCode::BAD_GATEWAY;
                error_log!(
                    REVERSE_PROXY_LOGGER_DOMAIN,
                    "playback_info_request_failed method={} path={} status={} \
                     error={}",
                    ctx.method,
                    ctx.path,
                    status.as_u16(),
                    error
                );
                return ResponseBuilder::with_status_code(status);
            }
        };

        match serde_json::to_string(&playback_info) {
            Ok(body_json) => {
                ResponseBuilder::with_json(StatusCode::OK, &body_json)
            }
            Err(error) => {
                error_log!(
                    REVERSE_PROXY_LOGGER_DOMAIN,
                    "Failed to serialize playback info response: {}",
                    error
                );
                ResponseBuilder::with_status_code(
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
            }
        }
    }
}

#[async_trait]
impl Middleware for ReverseProxyMiddleware {
    async fn handle(
        &self,
        ctx: Context,
        body: Option<Incoming>,
        _next: Next,
    ) -> Response<BoxBodyType> {
        debug_log!(
            REVERSE_PROXY_LOGGER_DOMAIN,
            "Starting reverse proxy middleware for {} {}",
            ctx.method,
            ctx.path
        );

        if ctx.path == ROOT_PATH {
            return ResponseBuilder::with_redirect(
                WEB_INDEX_REDIRECT,
                StatusCode::FOUND,
                None,
            );
        }

        if (ctx.method == Method::GET || ctx.method == Method::POST)
            && ctx.path.contains(PLAYBACK_INFO_PATH_SEGMENT)
        {
            return self.handle_playback_info_request(&ctx, body).await;
        }

        let cacheable_route =
            find_cacheable_route(&ctx.path, ctx.method.as_str());

        let body_bytes = Self::read_body(body).await;

        let cache_key = cacheable_route.and_then(|route| {
            Self::build_cache_key(&ctx, route, body_bytes.as_ref())
        });

        if let (Some(route), Some(key)) = (cacheable_route, cache_key) {
            if let Some(cached_response) = self.try_cache_hit(&key) {
                return cached_response;
            }

            let lock = self.lock_api_request(&key).await;
            let response = {
                let _guard = lock.lock().await;

                if let Some(cached_response) = self.try_cache_hit(&key) {
                    debug_log!(
                        API_CACHE_LOGGER_DOMAIN,
                        "[CACHE WAIT HIT] key={}{}",
                        key,
                        Self::cache_key_log_suffix(&key)
                    );
                    cached_response
                } else {
                    match self.proxy_and_read(&ctx, body_bytes).await {
                        Some((status, resp_headers, resp_body)) => {
                            if Self::should_cache_response(
                                status,
                                &resp_headers,
                            ) {
                                self.store_cache(
                                    key.clone(),
                                    status,
                                    &resp_headers,
                                    &resp_body,
                                    route.ttl_seconds,
                                );
                            }

                            Self::build_proxy_response(
                                status,
                                &resp_headers,
                                resp_body,
                            )
                        }
                        None => ResponseBuilder::with_status_code(
                            StatusCode::BAD_GATEWAY,
                        ),
                    }
                }
            };

            AppState::cleanup_request_lock(
                &self.state.api_request_locks,
                &key,
                &lock,
            );

            return response;
        }

        let Some((status, resp_headers, resp_body)) =
            self.proxy_and_read(&ctx, body_bytes).await
        else {
            return ResponseBuilder::with_status_code(StatusCode::BAD_GATEWAY);
        };

        Self::build_proxy_response(status, &resp_headers, resp_body)
    }

    fn clone_box(&self) -> Box<dyn Middleware> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use bytes::Bytes;
    use hyper::{Method, StatusCode, Uri};
    use regex::Regex;
    use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

    use super::{MAX_CACHEABLE_BODY_BYTES, ReverseProxyMiddleware};
    use crate::client::PlaybackInfoServiceError;
    use crate::gateway::{
        cacheable_routes::{
            BodyKeyStrategy, CacheKeyStrategy, CompiledCacheableRoute,
        },
        context::Context,
    };

    fn compiled_route(
        body_key_strategy: BodyKeyStrategy,
    ) -> CompiledCacheableRoute {
        CompiledCacheableRoute {
            regex: Regex::new(".*").unwrap_or_else(|_| unreachable!()),
            methods: &["POST"],
            ttl_seconds: 1,
            key_strategy: CacheKeyStrategy::FullUri,
            body_key_strategy,
        }
    }

    fn context_with_content_type(content_type: &str) -> Context {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(content_type)
                .unwrap_or_else(|_| unreachable!()),
        );

        Context::new(
            "/emby/Items/1/Action"
                .parse::<Uri>()
                .unwrap_or_else(|_| unreachable!()),
            Method::POST,
            headers,
            Instant::now(),
            "request-1".into(),
        )
    }

    #[test]
    fn build_target_url_collapses_double_slash_from_normalized_base() {
        // `Uri::to_string()` yields a trailing slash for authority-only URLs.
        let url = ReverseProxyMiddleware::build_target_url(
            "http://emby:8096/",
            "/emby/Users/3ef6a13df13e48b3ae594f22804150b8?reqformat=json",
        );

        assert_eq!(
            url,
            "http://emby:8096/emby/Users/3ef6a13df13e48b3ae594f22804150b8?reqformat=json"
        );
        assert!(!url.contains("8096//"));
    }

    #[test]
    fn build_target_url_keeps_single_slash_without_trailing_base_slash() {
        let url = ReverseProxyMiddleware::build_target_url(
            "http://emby:8096",
            "/emby/Items/1",
        );

        assert_eq!(url, "http://emby:8096/emby/Items/1");
    }

    #[test]
    fn build_target_url_inserts_slash_when_path_missing_leading_slash() {
        let url = ReverseProxyMiddleware::build_target_url(
            "http://emby:8096",
            "emby/Items/1",
        );

        assert_eq!(url, "http://emby:8096/emby/Items/1");
    }

    #[test]
    fn should_cache_json_success_response() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );

        assert!(ReverseProxyMiddleware::should_cache_response(
            StatusCode::OK,
            &headers
        ));
    }

    #[test]
    fn should_not_cache_non_json_success_response() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));

        assert!(!ReverseProxyMiddleware::should_cache_response(
            StatusCode::OK,
            &headers
        ));
    }

    #[test]
    fn should_not_cache_json_error_response() {
        let mut headers = HeaderMap::new();
        headers
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        assert!(!ReverseProxyMiddleware::should_cache_response(
            StatusCode::BAD_REQUEST,
            &headers
        ));
    }

    #[test]
    fn build_cache_key_normalizes_json_body_order() {
        let route = compiled_route(BodyKeyStrategy::AutoContentType);
        let ctx = context_with_content_type("application/json");
        let body1 = Bytes::from_static(br#"{"b":2,"a":1}"#);
        let body2 = Bytes::from_static(br#"{"a":1,"b":2}"#);

        let key1 =
            ReverseProxyMiddleware::build_cache_key(&ctx, &route, Some(&body1));
        let key2 =
            ReverseProxyMiddleware::build_cache_key(&ctx, &route, Some(&body2));

        assert_eq!(key1, key2);
        assert!(key1.is_some());
        assert!(key1.unwrap_or_default().contains(":body_hash:"));
    }

    #[test]
    fn build_cache_key_normalizes_form_body_order() {
        let route = compiled_route(BodyKeyStrategy::AutoContentType);
        let ctx =
            context_with_content_type("application/x-www-form-urlencoded");
        let body1 = Bytes::from_static(b"b=2&a=1");
        let body2 = Bytes::from_static(b"a=1&b=2");

        let key1 =
            ReverseProxyMiddleware::build_cache_key(&ctx, &route, Some(&body1));
        let key2 =
            ReverseProxyMiddleware::build_cache_key(&ctx, &route, Some(&body2));

        assert_eq!(key1, key2);
        assert!(key1.is_some());
        assert!(key1.unwrap_or_default().contains(":body_hash:"));
    }

    #[test]
    fn build_cache_key_skips_unknown_content_type() {
        let route = compiled_route(BodyKeyStrategy::AutoContentType);
        let ctx = context_with_content_type("application/octet-stream");
        let body = Bytes::from_static(b"binary-body");

        let key =
            ReverseProxyMiddleware::build_cache_key(&ctx, &route, Some(&body));

        assert!(key.is_none());
    }

    #[test]
    fn build_cache_key_skips_oversized_body() {
        let route = compiled_route(BodyKeyStrategy::AutoContentType);
        let ctx = context_with_content_type("application/json");
        let body = Bytes::from(vec![b'a'; MAX_CACHEABLE_BODY_BYTES + 1]);

        let key =
            ReverseProxyMiddleware::build_cache_key(&ctx, &route, Some(&body));

        assert!(key.is_none());
    }

    #[test]
    fn passthroughs_playback_info_parse_errors() {
        assert!(
            ReverseProxyMiddleware::should_passthrough_playback_info_error(
                &PlaybackInfoServiceError::InvalidItemId
            )
        );
        assert!(
            ReverseProxyMiddleware::should_passthrough_playback_info_error(
                &PlaybackInfoServiceError::InvalidMediaSourceId
            )
        );
        assert!(
            ReverseProxyMiddleware::should_passthrough_playback_info_error(
                &PlaybackInfoServiceError::UnsupportedMethod
            )
        );
        assert!(
            ReverseProxyMiddleware::should_passthrough_playback_info_error(
                &PlaybackInfoServiceError::EmptyApiToken
            )
        );
    }

    #[test]
    fn does_not_passthrough_playback_info_upstream_errors() {
        assert!(
            !ReverseProxyMiddleware::should_passthrough_playback_info_error(
                &PlaybackInfoServiceError::Upstream(anyhow::anyhow!(
                    "upstream"
                ))
            )
        );
    }
}
