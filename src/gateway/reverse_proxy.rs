use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::{
    Method, Response, StatusCode,
    body::Incoming,
    header::{self, HeaderName, HeaderValue},
};

use super::{
    chain::{Middleware, Next},
    context::Context,
    response::{BoxBodyType, ResponseBuilder},
};
use crate::{
    AppState, REVERSE_PROXY_LOGGER_DOMAIN,
    client::{PlaybackInfoRequest, PlaybackInfoService},
    debug_log, error_log, warn_log,
};

const ROOT_PATH: &str = "/";
const WEB_INDEX_REDIRECT: &str = "/web/index.html";
const PLAYBACK_INFO_PATH_SEGMENT: &str = "PlaybackInfo";

#[derive(Clone)]
pub struct ReverseProxyMiddleware {
    emby_base_url: String,
    http_client: reqwest::Client,
    state: Arc<AppState>,
    playback_info_service: PlaybackInfoService,
}

impl ReverseProxyMiddleware {
    pub fn new(emby_base_url: String, state: Arc<AppState>) -> Self {
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

    /// Serves PlaybackInfo with cache-aside semantics.
    ///
    /// On a cache hit the stored upstream JSON is replayed verbatim. On a miss
    /// the original request is proxied to Emby; a successful JSON response is
    /// cached (keyed by item id + normalized query) and its media sources are
    /// indexed by item id for later stream-path lookup before being
    /// returned to the client.
    async fn handle_playback_info_request(
        &self,
        ctx: &Context,
        body: Option<Incoming>,
    ) -> Response<BoxBodyType> {
        let body_bytes = Self::read_body(body).await;

        let Some(item_id) = PlaybackInfoRequest::item_id_from_path(&ctx.path)
        else {
            warn_log!(
                REVERSE_PROXY_LOGGER_DOMAIN,
                "playback_info_item_id_unparsed path={} fallback=proxy_to_emby",
                ctx.path
            );
            return self.proxy_playback_info_passthrough(ctx, body_bytes).await;
        };

        let cache_key =
            PlaybackInfoService::response_cache_key(&item_id, ctx.uri.query());

        if let Some(cached) =
            self.playback_info_service.cached_response(&cache_key).await
        {
            return ResponseBuilder::with_json(StatusCode::OK, &cached);
        }

        let lock = AppState::request_lock(
            &self.state.playback_info_request_locks,
            &cache_key,
        );

        let response = {
            let _guard = lock.lock().await;

            if let Some(cached) =
                self.playback_info_service.cached_response(&cache_key).await
            {
                ResponseBuilder::with_json(StatusCode::OK, &cached)
            } else {
                match self.proxy_and_read(ctx, body_bytes).await {
                    Some((status, resp_headers, resp_body)) => {
                        if Self::should_cache_response(status, &resp_headers) {
                            self.playback_info_service
                                .store_response_and_index(
                                    &cache_key, &item_id, &resp_body,
                                )
                                .await;
                        } else {
                            warn_log!(
                                REVERSE_PROXY_LOGGER_DOMAIN,
                                "playback_info_response_not_cached item_id={} \
                                 status={}",
                                item_id,
                                status.as_u16()
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
            &self.state.playback_info_request_locks,
            &cache_key,
            &lock,
        );

        response
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

        let body_bytes = Self::read_body(body).await;
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
    use hyper::StatusCode;
    use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

    use super::ReverseProxyMiddleware;

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
}
