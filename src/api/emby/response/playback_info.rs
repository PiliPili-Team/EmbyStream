use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PlaybackInfo {
    #[serde(rename = "MediaSources", default)]
    pub media_sources: Vec<MediaSource>,
    #[serde(rename = "PlaySessionId", default)]
    pub play_session_id: Option<String>,
}

impl PlaybackInfo {
    pub fn find_media_source_path_by_id(
        &self,
        target_id: &str,
    ) -> Option<&str> {
        self.media_sources
            .iter()
            .find(|source| source.id.as_deref() == Some(target_id))
            .and_then(|source| source.path.as_deref())
    }

    /// Selects the media source to stream for a forward request.
    ///
    /// A single PlaybackInfo response can describe several media sources (e.g.
    /// a multi-version movie), so the caller must identify which one to route.
    ///
    /// Selection policy:
    /// - When `media_source_id` is present, the source is matched by exact
    ///   `Id`; no positional guessing is performed.
    /// - When it is absent and the item exposes exactly one source, that source
    ///   is used unambiguously.
    /// - When it is absent and the item exposes multiple sources, the choice
    ///   is genuinely ambiguous and `None` is returned so the caller can reject
    ///   the request instead of streaming the wrong version.
    pub fn select_media_source(
        &self,
        media_source_id: Option<&str>,
    ) -> Option<&MediaSource> {
        match media_source_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(target_id) => self
                .media_sources
                .iter()
                .find(|source| source.id.as_deref() == Some(target_id)),
            None => match self.media_sources.as_slice() {
                [single] => Some(single),
                _ => None,
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MediaSource {
    #[serde(rename = "Protocol", default)]
    pub protocol: Option<String>,
    #[serde(rename = "Id", default)]
    pub id: Option<String>,
    #[serde(rename = "Path", default)]
    pub path: Option<String>,
    #[serde(rename = "ETag", default)]
    pub e_tag: Option<String>,
    #[serde(rename = "Type", default)]
    pub type_field: Option<String>,
    #[serde(rename = "Container", default)]
    pub container: Option<String>,
    #[serde(rename = "Size", default)]
    pub size: Option<i64>,
    #[serde(rename = "Name", default)]
    pub name: Option<String>,
    #[serde(rename = "IsRemote", default)]
    pub is_remote: Option<bool>,
    #[serde(rename = "HasMixedProtocols", default)]
    pub has_mixed_protocols: Option<bool>,
    #[serde(rename = "RunTimeTicks", default)]
    pub run_time_ticks: Option<i64>,
    #[serde(rename = "SupportsTranscoding", default)]
    pub supports_transcoding: Option<bool>,
    #[serde(rename = "SupportsDirectStream", default)]
    pub supports_direct_stream: Option<bool>,
    #[serde(rename = "SupportsDirectPlay", default)]
    pub supports_direct_play: Option<bool>,
    #[serde(rename = "IsInfiniteStream", default)]
    pub is_infinite_stream: Option<bool>,
    #[serde(rename = "RequiresOpening", default)]
    pub requires_opening: Option<bool>,
    #[serde(rename = "RequiresClosing", default)]
    pub requires_closing: Option<bool>,
    #[serde(rename = "RequiresLooping", default)]
    pub requires_looping: Option<bool>,
    #[serde(rename = "SupportsProbing", default)]
    pub supports_probing: Option<bool>,
    #[serde(rename = "MediaStreams", default)]
    pub media_streams: Vec<MediaStream>,
    #[serde(rename = "Formats", default)]
    pub formats: Vec<String>,
    #[serde(rename = "Bitrate", default)]
    pub bitrate: Option<i64>,
    #[serde(rename = "RequiredHttpHeaders", default)]
    pub required_http_headers: HashMap<String, String>,
    #[serde(rename = "AddApiKeyToDirectStreamUrl", default)]
    pub add_api_key_to_direct_stream_url: Option<bool>,
    #[serde(rename = "DirectStreamUrl", default)]
    pub direct_stream_url: Option<String>,
    #[serde(rename = "TranscodingUrl", default)]
    pub transcoding_url: Option<String>,
    #[serde(rename = "TranscodingSubProtocol", default)]
    pub transcoding_sub_protocol: Option<String>,
    #[serde(rename = "TranscodingContainer", default)]
    pub transcoding_container: Option<String>,
    #[serde(rename = "ReadAtNativeFramerate", default)]
    pub read_at_native_framerate: Option<bool>,
    #[serde(rename = "DefaultAudioStreamIndex", default)]
    pub default_audio_stream_index: Option<i32>,
    #[serde(rename = "DefaultSubtitleStreamIndex", default)]
    pub default_subtitle_stream_index: Option<i32>,
    #[serde(rename = "ItemId", default)]
    pub item_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MediaStream {
    #[serde(rename = "Codec", default)]
    pub codec: Option<String>,
    #[serde(rename = "CodecTag", default)]
    pub codec_tag: Option<String>,
    #[serde(rename = "Language", default)]
    pub language: Option<String>,
    #[serde(rename = "Title", default)]
    pub title: Option<String>,
    #[serde(rename = "ColorTransfer", default)]
    pub color_transfer: Option<String>,
    #[serde(rename = "ColorPrimaries", default)]
    pub color_primaries: Option<String>,
    #[serde(rename = "ColorSpace", default)]
    pub color_space: Option<String>,
    #[serde(rename = "TimeBase", default)]
    pub time_base: Option<String>,
    #[serde(rename = "VideoRange", default)]
    pub video_range: Option<String>,
    #[serde(rename = "DisplayTitle", default)]
    pub display_title: Option<String>,
    #[serde(rename = "IsInterlaced", default)]
    pub is_interlaced: Option<bool>,
    #[serde(rename = "BitRate", default)]
    pub bit_rate: Option<i64>,
    #[serde(rename = "BitDepth", default)]
    pub bit_depth: Option<i32>,
    #[serde(rename = "RefFrames", default)]
    pub ref_frames: Option<i32>,
    #[serde(rename = "IsDefault", default)]
    pub is_default: Option<bool>,
    #[serde(rename = "IsForced", default)]
    pub is_forced: Option<bool>,
    #[serde(rename = "IsHearingImpaired", default)]
    pub is_hearing_impaired: Option<bool>,
    #[serde(rename = "Height", default)]
    pub height: Option<i32>,
    #[serde(rename = "Width", default)]
    pub width: Option<i32>,
    #[serde(rename = "AverageFrameRate", default)]
    pub average_frame_rate: Option<f32>,
    #[serde(rename = "RealFrameRate", default)]
    pub real_frame_rate: Option<f32>,
    #[serde(rename = "Profile", default)]
    pub profile: Option<String>,
    #[serde(rename = "Type", default)]
    pub type_field: Option<String>,
    #[serde(rename = "AspectRatio", default)]
    pub aspect_ratio: Option<String>,
    #[serde(rename = "Index", default)]
    pub index: Option<i32>,
    #[serde(rename = "IsExternal", default)]
    pub is_external: Option<bool>,
    #[serde(rename = "IsExternalUrl", default)]
    pub is_external_url: Option<bool>,
    #[serde(rename = "IsTextSubtitleStream", default)]
    pub is_text_subtitle_stream: Option<bool>,
    #[serde(rename = "SupportsExternalStream", default)]
    pub supports_external_stream: Option<bool>,
    #[serde(rename = "Path", default)]
    pub path: Option<String>,
    #[serde(rename = "DeliveryMethod", default)]
    pub delivery_method: Option<String>,
    #[serde(rename = "DeliveryUrl", default)]
    pub delivery_url: Option<String>,
    #[serde(rename = "Protocol", default)]
    pub protocol: Option<String>,
    #[serde(rename = "PixelFormat", default)]
    pub pixel_format: Option<String>,
    #[serde(rename = "Level", default)]
    pub level: Option<i32>,
    #[serde(rename = "IsAnamorphic", default)]
    pub is_anamorphic: Option<bool>,
    #[serde(rename = "ExtendedVideoType", default)]
    pub extended_video_type: Option<String>,
    #[serde(rename = "ExtendedVideoSubType", default)]
    pub extended_video_sub_type: Option<String>,
    #[serde(rename = "ExtendedVideoSubTypeDescription", default)]
    pub extended_video_sub_type_description: Option<String>,
    #[serde(rename = "AttachmentSize", default)]
    pub attachment_size: Option<i32>,
    #[serde(rename = "ChannelLayout", default)]
    pub channel_layout: Option<String>,
    #[serde(rename = "Channels", default)]
    pub channels: Option<i32>,
    #[serde(rename = "SampleRate", default)]
    pub sample_rate: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PlaybackInfo ──────────────────────────────────────────────────────────

    #[test]
    fn test_playback_info_deserialize_full() {
        let json = r#"{
            "MediaSources": [{
                "Id": "mediasource_1",
                "Path": "/media/video.mkv",
                "DirectStreamUrl": "/emby/videos/1/stream.mkv?MediaSourceId=mediasource_1&Static=true",
                "TranscodingUrl": "/emby/videos/1/stream.m3u8?MediaSourceId=mediasource_1",
                "TranscodingSubProtocol": "hls",
                "TranscodingContainer": "ts"
            }],
            "PlaySessionId": "session-abc"
        }"#;

        let info: PlaybackInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.play_session_id.as_deref(), Some("session-abc"));
        assert_eq!(info.media_sources.len(), 1);

        let src = &info.media_sources[0];
        assert_eq!(src.id.as_deref(), Some("mediasource_1"));
        assert_eq!(src.path.as_deref(), Some("/media/video.mkv"));
        assert_eq!(
            src.direct_stream_url.as_deref(),
            Some(
                "/emby/videos/1/stream.mkv?MediaSourceId=mediasource_1&Static=true"
            )
        );
        assert_eq!(
            src.transcoding_url.as_deref(),
            Some("/emby/videos/1/stream.m3u8?MediaSourceId=mediasource_1")
        );
        assert_eq!(src.transcoding_sub_protocol.as_deref(), Some("hls"));
        assert_eq!(src.transcoding_container.as_deref(), Some("ts"));
    }

    #[test]
    fn test_playback_info_deserialize_minimal() {
        let json = r#"{"MediaSources": [], "PlaySessionId": null}"#;
        let info: PlaybackInfo = serde_json::from_str(json).unwrap();
        assert!(info.media_sources.is_empty());
        assert!(info.play_session_id.is_none());
    }

    #[test]
    fn test_playback_info_deserialize_empty_object() {
        let info: PlaybackInfo = serde_json::from_str("{}").unwrap();
        assert!(info.media_sources.is_empty());
        assert!(info.play_session_id.is_none());
    }

    #[test]
    fn test_playback_info_find_media_source_path_by_id_found() {
        let info = PlaybackInfo {
            media_sources: vec![
                MediaSource {
                    id: Some("src-a".into()),
                    path: Some("/media/a.mkv".into()),
                    ..Default::default()
                },
                MediaSource {
                    id: Some("src-b".into()),
                    path: Some("/media/b.mkv".into()),
                    ..Default::default()
                },
            ],
            play_session_id: None,
        };
        assert_eq!(
            info.find_media_source_path_by_id("src-b"),
            Some("/media/b.mkv")
        );
    }

    #[test]
    fn test_playback_info_find_media_source_path_by_id_not_found() {
        let info = PlaybackInfo {
            media_sources: vec![MediaSource {
                id: Some("src-a".into()),
                path: Some("/media/a.mkv".into()),
                ..Default::default()
            }],
            play_session_id: None,
        };
        assert!(info.find_media_source_path_by_id("src-z").is_none());
    }

    #[test]
    fn test_playback_info_find_media_source_path_by_id_no_path() {
        let info = PlaybackInfo {
            media_sources: vec![MediaSource {
                id: Some("src-a".into()),
                path: None,
                ..Default::default()
            }],
            play_session_id: None,
        };
        assert!(info.find_media_source_path_by_id("src-a").is_none());
    }

    fn multi_source_info() -> PlaybackInfo {
        PlaybackInfo {
            media_sources: vec![
                MediaSource {
                    id: Some("src-a".into()),
                    path: Some("/media/a.mkv".into()),
                    ..Default::default()
                },
                MediaSource {
                    id: Some("src-b".into()),
                    path: Some("/media/b.mkv".into()),
                    ..Default::default()
                },
            ],
            play_session_id: None,
        }
    }

    fn single_source_info() -> PlaybackInfo {
        PlaybackInfo {
            media_sources: vec![MediaSource {
                id: Some("src-a".into()),
                path: Some("/media/a.mkv".into()),
                ..Default::default()
            }],
            play_session_id: None,
        }
    }

    #[test]
    fn test_select_media_source_matches_by_id() {
        let info = multi_source_info();
        let source = info.select_media_source(Some("src-b"));
        assert_eq!(
            source.and_then(|s| s.path.as_deref()),
            Some("/media/b.mkv")
        );
    }

    #[test]
    fn test_select_media_source_unknown_id_returns_none() {
        let info = multi_source_info();
        assert!(info.select_media_source(Some("src-z")).is_none());
    }

    #[test]
    fn test_select_media_source_single_source_without_id() {
        let info = single_source_info();
        let source = info.select_media_source(None);
        assert_eq!(
            source.and_then(|s| s.path.as_deref()),
            Some("/media/a.mkv")
        );
    }

    #[test]
    fn test_select_media_source_single_source_blank_id() {
        // A blank MediaSourceId is treated as absent.
        let info = single_source_info();
        let source = info.select_media_source(Some("   "));
        assert_eq!(source.and_then(|s| s.id.as_deref()), Some("src-a"));
    }

    #[test]
    fn test_select_media_source_multi_source_without_id_is_ambiguous() {
        let info = multi_source_info();
        // Multiple sources with no MediaSourceId must not be guessed.
        assert!(info.select_media_source(None).is_none());
    }

    #[test]
    fn test_select_media_source_empty_returns_none() {
        let info = PlaybackInfo::default();
        assert!(info.select_media_source(None).is_none());
        assert!(info.select_media_source(Some("src-a")).is_none());
    }

    // ── MediaSource ───────────────────────────────────────────────────────────

    #[test]
    fn test_media_source_direct_stream_url_round_trip() {
        let url = "/emby/videos/20690/stream.mkv?\
                   MediaSourceId=mediasource_20690&Static=true&api_key=xxx";
        let src = MediaSource {
            id: Some("mediasource_20690".into()),
            direct_stream_url: Some(url.into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&src).unwrap();
        let restored: MediaSource = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.direct_stream_url.as_deref(), Some(url));
    }

    #[test]
    fn test_media_source_transcoding_fields_round_trip() {
        let src = MediaSource {
            transcoding_url: Some(
                "/emby/videos/1/stream.m3u8?MediaSourceId=src1".into(),
            ),
            transcoding_sub_protocol: Some("hls".into()),
            transcoding_container: Some("ts".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&src).unwrap();
        let restored: MediaSource = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.transcoding_sub_protocol.as_deref(), Some("hls"));
        assert_eq!(restored.transcoding_container.as_deref(), Some("ts"));
    }

    #[test]
    fn test_media_source_subtitle_index_round_trip() {
        let src = MediaSource {
            default_audio_stream_index: Some(1),
            default_subtitle_stream_index: Some(3),
            ..Default::default()
        };
        let json = serde_json::to_string(&src).unwrap();
        let restored: MediaSource = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.default_audio_stream_index, Some(1));
        assert_eq!(restored.default_subtitle_stream_index, Some(3));
    }

    #[test]
    fn test_media_source_e_tag_round_trip() {
        let src = MediaSource {
            e_tag: Some("abc123etag".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&src).unwrap();
        let restored: MediaSource = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.e_tag.as_deref(), Some("abc123etag"));
    }

    #[test]
    fn test_media_source_missing_optional_fields_default_to_none() {
        let json = r#"{"Id": "src-1", "Container": "mkv"}"#;
        let src: MediaSource = serde_json::from_str(json).unwrap();
        assert!(src.direct_stream_url.is_none());
        assert!(src.transcoding_url.is_none());
        assert!(src.transcoding_sub_protocol.is_none());
        assert!(src.transcoding_container.is_none());
        assert!(src.default_subtitle_stream_index.is_none());
        assert!(src.e_tag.is_none());
    }

    #[test]
    fn test_media_source_deserialize_empty_object() {
        let src: MediaSource = serde_json::from_str("{}").unwrap();
        assert!(src.id.is_none());
        assert!(src.direct_stream_url.is_none());
        assert!(src.transcoding_url.is_none());
        assert!(src.media_streams.is_empty());
        assert!(src.formats.is_empty());
        assert!(src.required_http_headers.is_empty());
    }

    // ── MediaStream ───────────────────────────────────────────────────────────

    #[test]
    fn test_media_stream_delivery_fields_round_trip() {
        let stream = MediaStream {
            delivery_method: Some("External".into()),
            delivery_url: Some("/Subtitles/1/0/Stream.srt?api_key=xxx".into()),
            is_external_url: Some(false),
            ..Default::default()
        };
        let json = serde_json::to_string(&stream).unwrap();
        let restored: MediaStream = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.delivery_method.as_deref(), Some("External"));
        assert_eq!(
            restored.delivery_url.as_deref(),
            Some("/Subtitles/1/0/Stream.srt?api_key=xxx")
        );
        assert_eq!(restored.is_external_url, Some(false));
    }

    #[test]
    fn test_media_stream_title_and_path_round_trip() {
        let stream = MediaStream {
            title: Some("Forced English".into()),
            path: Some("/subs/en_forced.srt".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&stream).unwrap();
        let restored: MediaStream = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.title.as_deref(), Some("Forced English"));
        assert_eq!(restored.path.as_deref(), Some("/subs/en_forced.srt"));
    }

    #[test]
    fn test_media_stream_missing_optional_fields_default_to_none() {
        let json = r#"{"Codec": "hevc", "Type": "Video"}"#;
        let stream: MediaStream = serde_json::from_str(json).unwrap();
        assert!(stream.delivery_method.is_none());
        assert!(stream.delivery_url.is_none());
        assert!(stream.is_external_url.is_none());
        assert!(stream.title.is_none());
        assert!(stream.path.is_none());
    }

    #[test]
    fn test_media_stream_deserialize_empty_object() {
        let stream: MediaStream = serde_json::from_str("{}").unwrap();
        assert!(stream.codec.is_none());
        assert!(stream.delivery_url.is_none());
        assert!(stream.title.is_none());
    }
}
