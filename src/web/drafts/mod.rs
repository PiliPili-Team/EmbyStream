use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, patch, post},
};
use axum_extra::extract::CookieJar;

use crate::{
    cli_wizard::{
        emit::emit_wizard_config_toml, template_payload::build_template_raw,
    },
    config::{core::finish_raw_config, general::General, types::RawConfig},
    web::{
        api::WebAppState,
        artifacts::render_all,
        auth::session_user_from_jar,
        contracts::{
            ConfigSetListResponse, CreateDraftRequest, DraftDocumentEnvelope,
            DraftEnvelope, DraftListResponse, GenerateDraftResponse,
            MetadataUpdateRequest, SaveDraftRequest, SaveDraftResponse,
            WizardBackendNginxPayload, WizardDeploymentPayload,
            WizardFrontendNginxPayload, WizardNginxPayload, WizardPayload,
            WizardPm2Payload, WizardSharedGeneral, WizardSharedPayload,
            WizardStreamMode, WizardSystemdPayload, WizardTemplateResponse,
        },
        db::PersistGeneratedConfigInput,
        error::WebError,
    },
};

pub fn routes() -> Router<WebAppState> {
    Router::new()
        .route("/templates/{stream_mode}", get(get_wizard_template))
        .route("/", post(create_draft).get(list_drafts))
        .route(
            "/{draft_id}",
            get(get_draft).put(save_draft).delete(delete_draft),
        )
        .route("/{draft_id}/metadata", patch(update_draft_metadata))
        .route("/{draft_id}/generate", post(generate_from_draft))
}

pub async fn list_config_sets(
    State(state): State<WebAppState>,
    jar: CookieJar,
) -> Result<Json<ConfigSetListResponse>, WebError> {
    let user = session_user_from_jar(&state, &jar).await?;
    let items = state.db.list_config_sets(&user.id).await?;
    Ok(Json(ConfigSetListResponse { items }))
}

pub async fn create_draft(
    State(state): State<WebAppState>,
    jar: CookieJar,
    Json(payload): Json<CreateDraftRequest>,
) -> Result<Json<DraftEnvelope>, WebError> {
    let user = session_user_from_jar(&state, &jar).await?;
    let stream_mode = payload.stream_mode;
    let name = normalize_name(payload.name, stream_mode);
    let template = build_template_raw(stream_mode.into());
    let wizard_payload =
        clear_default_backend_nodes(wizard_payload_from_raw(template));

    let draft = state
        .db
        .create_draft(&user.id, name, stream_mode, wizard_payload)
        .await?;
    Ok(Json(DraftEnvelope { draft }))
}

pub async fn get_wizard_template(
    State(state): State<WebAppState>,
    jar: CookieJar,
    Path(stream_mode): Path<WizardStreamMode>,
) -> Result<Json<WizardTemplateResponse>, WebError> {
    let _user = session_user_from_jar(&state, &jar).await?;
    let template = build_template_raw(stream_mode.into());

    Ok(Json(WizardTemplateResponse {
        payload: clear_default_backend_nodes(wizard_payload_from_raw(template)),
    }))
}

pub async fn list_drafts(
    State(state): State<WebAppState>,
    jar: CookieJar,
) -> Result<Json<DraftListResponse>, WebError> {
    let user = session_user_from_jar(&state, &jar).await?;
    let items = state.db.list_drafts(&user.id).await?;
    Ok(Json(DraftListResponse { items }))
}

pub async fn get_draft(
    State(state): State<WebAppState>,
    jar: CookieJar,
    Path(draft_id): Path<String>,
) -> Result<Json<DraftDocumentEnvelope>, WebError> {
    let user = session_user_from_jar(&state, &jar).await?;
    let draft = state
        .db
        .get_draft(&user.id, &draft_id)
        .await?
        .ok_or(WebError::NotFound("Draft was not found."))?;
    Ok(Json(DraftDocumentEnvelope { draft }))
}

pub async fn save_draft(
    State(state): State<WebAppState>,
    jar: CookieJar,
    Path(draft_id): Path<String>,
    Json(payload): Json<SaveDraftRequest>,
) -> Result<Json<SaveDraftResponse>, WebError> {
    let user = session_user_from_jar(&state, &jar).await?;
    let name = normalize_name(payload.name, payload.payload.stream_mode);
    let revision = state
        .db
        .save_draft(
            &user.id,
            &draft_id,
            name,
            payload.payload,
            payload.client_revision,
        )
        .await?;
    Ok(Json(SaveDraftResponse { draft: revision }))
}

pub async fn delete_draft(
    State(state): State<WebAppState>,
    jar: CookieJar,
    Path(draft_id): Path<String>,
) -> Result<Json<super::contracts::LogoutResponse>, WebError> {
    let user = session_user_from_jar(&state, &jar).await?;
    state.db.delete_draft(&user.id, &draft_id).await?;
    Ok(Json(super::contracts::LogoutResponse { ok: true }))
}

pub async fn update_draft_metadata(
    State(state): State<WebAppState>,
    jar: CookieJar,
    Path(draft_id): Path<String>,
    Json(payload): Json<MetadataUpdateRequest>,
) -> Result<Json<DraftEnvelope>, WebError> {
    let user = session_user_from_jar(&state, &jar).await?;
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(WebError::invalid_input("name", "Draft name is required."));
    }

    let draft = state
        .db
        .update_draft_metadata(&user.id, &draft_id, name)
        .await?
        .ok_or(WebError::NotFound("Draft was not found."))?;
    Ok(Json(DraftEnvelope { draft }))
}

pub async fn generate_from_draft(
    State(state): State<WebAppState>,
    jar: CookieJar,
    Path(draft_id): Path<String>,
) -> Result<Json<GenerateDraftResponse>, WebError> {
    let user = session_user_from_jar(&state, &jar).await?;
    let draft = state
        .db
        .get_draft(&user.id, &draft_id)
        .await?
        .ok_or(WebError::NotFound("Draft was not found."))?;

    let raw = raw_from_wizard_payload(&draft.payload);
    finish_raw_config(PathBuf::from("web-generated.toml"), raw.clone())
        .map_err(|error| WebError::ValidationFailed(error.to_string()))?;
    let config_toml = emit_wizard_config_toml(&raw)
        .map_err(|error| WebError::internal(error.to_string()))?;
    let rendered = render_all(&raw, &draft.payload, config_toml.clone());

    let response = state
        .db
        .persist_generated_config(PersistGeneratedConfigInput {
            user_id: user.id,
            draft_id,
            draft_name: draft.name,
            payload: draft.payload,
            stream_mode: draft.stream_mode,
            config_toml,
            artifacts: rendered,
        })
        .await?;

    Ok(Json(response))
}

pub fn raw_from_wizard_payload(payload: &WizardPayload) -> RawConfig {
    RawConfig {
        general: General {
            memory_mode: payload.shared.general.memory_mode.clone(),
            stream_mode: payload.stream_mode.into(),
            encipher_key: payload.shared.general.encipher_key.clone(),
            encipher_iv: payload.shared.general.encipher_iv.clone(),
        },
        log: payload.shared.log.clone(),
        emby: payload.shared.emby.clone(),
        user_agent: payload.shared.user_agent.clone(),
        http2: Some(payload.shared.http2.clone()),
        frontend: payload.frontend.clone(),
        backend: payload.backend.clone(),
        backend_nodes: Some(payload.backend_nodes.clone()),
        disk: None,
        open_list: None,
        direct_link: None,
        fallback: payload.shared.fallback.clone(),
    }
}

pub fn wizard_payload_from_raw(raw: RawConfig) -> WizardPayload {
    let nginx = default_nginx_payload_from_raw(&raw);
    WizardPayload {
        stream_mode: raw.general.stream_mode.into(),
        shared: WizardSharedPayload {
            log: raw.log,
            general: WizardSharedGeneral {
                memory_mode: raw.general.memory_mode,
                encipher_key: raw.general.encipher_key,
                encipher_iv: raw.general.encipher_iv,
            },
            emby: raw.emby,
            user_agent: raw.user_agent,
            fallback: raw.fallback,
            http2: raw.http2.unwrap_or_default(),
        },
        frontend: raw.frontend,
        backend: raw.backend,
        backend_nodes: raw.backend_nodes.unwrap_or_default(),
        nginx,
        deployment: default_deployment_payload(raw.general.stream_mode.into()),
    }
}

fn clear_default_backend_nodes(mut payload: WizardPayload) -> WizardPayload {
    if payload.stream_mode != WizardStreamMode::Frontend {
        payload.backend_nodes.clear();
    }
    payload
}

fn strip_scheme_host(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_string()
}

fn default_nginx_payload_from_raw(raw: &RawConfig) -> WizardNginxPayload {
    let frontend_server_name = raw
        .frontend
        .as_ref()
        .and_then(|frontend| {
            frontend
                .anti_reverse_proxy
                .trusted_hosts
                .first()
                .map(|host| strip_scheme_host(host))
        })
        .filter(|value| !value.is_empty())
        .or_else(|| {
            raw.backend
                .as_ref()
                .map(|backend| strip_scheme_host(&backend.base_url))
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "stream.example.com".to_string());

    let backend_server_name = raw
        .backend
        .as_ref()
        .map(|backend| strip_scheme_host(&backend.base_url))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| frontend_server_name.clone());

    WizardNginxPayload {
        frontend: WizardFrontendNginxPayload {
            server_name: frontend_server_name,
            ..Default::default()
        },
        backend: WizardBackendNginxPayload {
            server_name: backend_server_name,
            ..Default::default()
        },
    }
}

fn default_deployment_payload(
    stream_mode: WizardStreamMode,
) -> WizardDeploymentPayload {
    let pm2_working_directory = match stream_mode {
        WizardStreamMode::Frontend => "/opt/stream-frontend",
        WizardStreamMode::Backend => "/opt/stream-backend",
        WizardStreamMode::Dual => "/opt/stream",
    };

    WizardDeploymentPayload {
        systemd: WizardSystemdPayload {
            binary_path: "/usr/bin/embystream".to_string(),
            working_directory: "/opt/stream".to_string(),
            config_path: "/opt/stream/config.toml".to_string(),
        },
        pm2: WizardPm2Payload {
            binary_path: "/usr/bin/embystream".to_string(),
            working_directory: pm2_working_directory.to_string(),
            config_path: format!("{pm2_working_directory}/config.toml"),
            out_file: format!("{pm2_working_directory}/logs/pm2.out.log"),
            error_file: format!("{pm2_working_directory}/logs/pm2.err.log"),
        },
    }
}

fn normalize_name(name: String, stream_mode: WizardStreamMode) -> String {
    let trimmed = name.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    let default_name = match stream_mode {
        WizardStreamMode::Frontend => "Frontend setup",
        WizardStreamMode::Backend => "Backend setup",
        WizardStreamMode::Dual => "Dual setup",
    };
    default_name.to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        cli_wizard::{
            emit::emit_wizard_config_toml, template_payload::build_template_raw,
        },
        config::general::StreamMode,
        web::contracts::SaveDraftRequest,
    };

    use super::{
        clear_default_backend_nodes, raw_from_wizard_payload,
        wizard_payload_from_raw,
    };

    #[test]
    fn wizard_payload_round_trip_preserves_generated_config() {
        let raw = build_template_raw(StreamMode::Frontend);
        let payload = wizard_payload_from_raw(raw.clone());
        let round_tripped = raw_from_wizard_payload(&payload);

        let original_toml =
            emit_wizard_config_toml(&raw).expect("original toml");
        let round_tripped_toml = emit_wizard_config_toml(&round_tripped)
            .expect("round tripped toml");

        assert_eq!(original_toml, round_tripped_toml);
    }

    #[test]
    fn save_draft_request_accepts_web_backend_node_field_names() {
        let payload = json!({
            "name": "Demo",
            "payload": {
                "stream_mode": "backend",
                "shared": {
                    "log": {
                        "level": "info",
                        "prefix": "demo",
                        "root_path": "./logs"
                    },
                    "general": {
                        "memory_mode": "middle",
                        "encipher_key": "RandomSecretKey1",
                        "encipher_iv": "RandomSecretIv2"
                    },
                    "emby": {
                        "url": "http://127.0.0.1",
                        "port": "8096",
                        "token": ""
                    },
                    "user_agent": {
                        "mode": "deny",
                        "allow_ua": [],
                        "deny_ua": ["curl"]
                    },
                    "fallback": {
                        "video_missing_path": "/mnt/anime/fallback/video_missing.mp4"
                    },
                    "http2": {
                        "ssl_cert_file": "",
                        "ssl_key_file": ""
                    }
                },
                "frontend": null,
                "backend": {
                    "listen_port": 60001,
                    "base_url": "https://backend.example.com",
                    "port": "443",
                    "path": "stream",
                    "check_file_existence": true,
                    "problematic_clients": ["Emby/"]
                },
                "backend_nodes": [{
                    "name": "LocalDisk",
                    "backend_type": "Disk",
                    "pattern": "/mnt/media/.*",
                    "base_url": "http://127.0.0.1",
                    "port": "60002",
                    "path": "",
                    "priority": 0,
                    "proxy_mode": "proxy",
                    "client_speed_limit_kbs": 0,
                    "client_burst_speed_kbs": 0,
                    "path_rewrites": [{
                        "enable": false,
                        "pattern": "^/mnt/media(/.*)$",
                        "replacement": "/media$1"
                    }],
                    "anti_reverse_proxy": {
                        "enable": false,
                        "host": ""
                    },
                    "disk": {
                        "description": ""
                    },
                    "open_list": null,
                    "direct_link": null,
                    "google_drive": null,
                    "webdav": null
                }],
                "nginx": {
                    "frontend": {},
                    "backend": {}
                },
                "deployment": {
                    "systemd": {},
                    "pm2": {}
                }
            },
            "client_revision": 1
        });

        let request: SaveDraftRequest =
            serde_json::from_value(payload).expect("parse save draft request");

        let node = &request.payload.backend_nodes[0];
        assert_eq!(node.backend_type, "Disk");
        assert_eq!(node.path_rewrites.len(), 1);
        assert!(node.anti_reverse_proxy.trusted_hosts.is_empty());
        assert!(node.disk.is_some());
    }

    #[test]
    fn wizard_templates_start_with_empty_backend_nodes() {
        let backend = clear_default_backend_nodes(wizard_payload_from_raw(
            build_template_raw(StreamMode::Backend),
        ));
        let dual = clear_default_backend_nodes(wizard_payload_from_raw(
            build_template_raw(StreamMode::Dual),
        ));

        assert!(backend.backend_nodes.is_empty());
        assert!(dual.backend_nodes.is_empty());
    }
}
