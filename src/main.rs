use std::{
    error::Error, fs, io::Error as IoError, path::Path, path::PathBuf, process,
    str::FromStr, sync::Arc,
};

use clap::{CommandFactory, FromArgMatches};
use figlet_rs::FIGfont;
use hyper::{StatusCode, body::Incoming};
use tokio::signal as TokioSignal;

use embystream::{
    AppState, GATEWAY_LOGGER_DOMAIN, INIT_LOGGER_DOMAIN, debug_log, error_log,
    i18n::lookup, info_log,
};
use embystream::{
    auth::google::{GoogleAuthArgs, run_google_auth},
    backend::{
        google_drive_auth, service::AppStreamService, stream::StreamMiddleware,
        stream_relay::StreamRelayMiddleware,
    },
    cli::{
        AuthSubcommand, Cli, Commands, RunArgs, WebAdminSubcommand, WebArgs,
    },
    cli_lang::{detect_lang_from_env_early, localize_cli_command},
    cli_wizard,
    config::{
        core::{Config, LoadConfigOutcome},
        general::StreamMode,
    },
    frontend::{forward::ForwardMiddleware, service::AppForwardService},
    gateway::{
        CorsMiddleware, LoggerMiddleware, OptionsMiddleware,
        PlaylistMockMiddleware, ReverseProxyMiddleware, chain::Handler,
        client_filter::ClientAgentFilterMiddleware, context::Context,
        core::Gateway, filtered_routes::COMPILED_UA_FILTERS,
        response::ResponseBuilder,
        reverse_proxy_filter::ReverseProxyFilterMiddleware,
    },
    log_stream::global_log_stream,
    logger::{LogLevel, Logger, start_cleanup_task},
    system::SystemInfo,
    web::{
        app::{WebRuntimeConfig, serve_web_app, to_runtime_config},
        db::Database,
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let help_lang = detect_lang_from_env_early();
    let mut cmd = Cli::command();
    localize_cli_command(&mut cmd, help_lang);
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());

    match cli.command {
        Some(Commands::Run(run_args)) => {
            run_app(&run_args).await?;
        }
        Some(Commands::Web(WebArgs { sub })) => match sub {
            embystream::cli::WebSubcommand::Serve(args) => {
                serve_web_app(to_runtime_config(args)?).await?;
            }
            embystream::cli::WebSubcommand::Admin(args) => match args.sub {
                WebAdminSubcommand::ResetPassword(args) => {
                    let db = Database::new(args.data_dir);
                    db.initialize().await?;
                    let password =
                        db.reset_admin_password(&args.username).await?;
                    println!(
                        "Administrator password reset for '{}': {}",
                        args.username, password
                    );
                }
            },
        },
        Some(Commands::Auth(auth_args)) => match auth_args.sub {
            AuthSubcommand::Google(args) => {
                run_google_auth(&GoogleAuthArgs {
                    client_id: args.client_id,
                    client_secret: args.client_secret,
                    no_browser: args.no_browser,
                })
                .await?;
            }
        },
        Some(Commands::Config(ref cfg_args)) => {
            if let Err(e) = cli_wizard::run(cfg_args, cli.lang) {
                let prefix = lookup(cli.lang, "error.wizard_prefix");
                eprintln!("{prefix}: {e}");
                process::exit(1);
            }
        }
        None => {}
    }
    Ok(())
}

async fn run_app(
    run_args: &RunArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    setup_figlet();

    let web_enabled = web_studio_enabled(run_args);

    match setup_load_config(run_args) {
        Ok(LoadConfigOutcome::Loaded(config)) => {
            let config = *config;
            if web_enabled {
                start_web_service(build_run_web_runtime_config(
                    run_args,
                    Some(&config),
                )?);
            }

            if let Err(error) = initialize_stream_runtime(&config).await {
                if web_enabled {
                    eprintln!(
                        "Stream startup skipped because the runtime config is not ready: {}",
                        error
                    );
                    if let Some(config_path) = &run_args.config {
                        eprintln!(
                            "Complete the configuration in the web studio, then rerun embystream run --config {} --web",
                            config_path.display()
                        );
                    } else {
                        eprintln!(
                            "Complete the configuration in the web studio, then rerun embystream run --web"
                        );
                    }
                } else {
                    return Err(error);
                }
            }
        }
        Ok(LoadConfigOutcome::TemplateCreated(config_path)) => {
            if web_enabled {
                start_web_service(build_run_web_runtime_config(
                    run_args, None,
                )?);
                println!(
                    "Stream startup skipped because no config was found. A template was created at '{}'",
                    config_path.display()
                );
                println!(
                    "Finish the setup in the web studio, then rerun embystream run --config {} --web",
                    config_path.display()
                );
            } else {
                eprintln!(
                    "A config template was created at '{}'",
                    config_path.display()
                );
                eprintln!(
                    "Please configure it and rerun embystream run --config {}",
                    config_path.display()
                );
                process::exit(0);
            }
        }
        Err(error) => {
            if web_enabled {
                start_web_service(build_run_web_runtime_config(
                    run_args, None,
                )?);
                eprintln!(
                    "Stream startup skipped because the configuration could not be loaded: {}",
                    error
                );
                eprintln!(
                    "Fix the configuration in the web studio, then rerun embystream run --web"
                );
            } else {
                eprintln!("Configuration initialization failed: {}", error);
                process::exit(1);
            }
        }
    }

    TokioSignal::ctrl_c().await?;
    info_log!(INIT_LOGGER_DOMAIN, "Shutting down EmbyStream...");

    Ok(())
}

async fn initialize_stream_runtime(
    config: &Config,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    setup_logger(config)?;
    setup_print_info(config);

    validate_dual_mode_ports(config)
        .map_err(|error| IoError::other(error.to_string()))?;

    setup_crypto_provider()?;

    let app_state = setup_cache(config).await;

    setup_rate_limiters(&app_state).await;
    setup_google_drive_refresh(&app_state).await;

    let mode = {
        let config_guard = app_state.get_config().await;
        config_guard.general.stream_mode
    };

    if matches!(mode, StreamMode::Frontend | StreamMode::Dual) {
        let frontend_state = app_state.clone();
        tokio::spawn(async move {
            if let Err(e) = setup_frontend_gateway(&frontend_state).await {
                error_log!(
                    INIT_LOGGER_DOMAIN,
                    "Frontend gateway failed: {}",
                    e
                );
            }
        });
    }

    if matches!(mode, StreamMode::Backend | StreamMode::Dual) {
        let backend_state = app_state.clone();
        tokio::spawn(async move {
            if let Err(e) = setup_backend_gateway(&backend_state).await {
                error_log!(INIT_LOGGER_DOMAIN, "Backend gateway failed: {}", e);
            }
        });
    }

    Ok(())
}

fn setup_figlet() {
    if let Ok(standard_font) = FIGfont::standard() {
        if let Some(figure) = standard_font.convert("EMBYSTREAM") {
            println!("{figure}");
        }
    }
}

fn setup_print_info(config: &Config) {
    info_log!(INIT_LOGGER_DOMAIN, "Initializing EmbyStream...");

    let system_info = SystemInfo::new();
    let configurarion = if cfg!(debug_assertions) {
        "Development"
    } else {
        "Production"
    };
    info_log!(
        INIT_LOGGER_DOMAIN,
        "Environment: {:?} [{:?}], Version: {:?}",
        system_info.environment,
        &configurarion,
        system_info.version
    );
    info_log!(
        INIT_LOGGER_DOMAIN,
        "Log level: {}",
        config.log.level.as_str()
    );
    info_log!(
        INIT_LOGGER_DOMAIN,
        "Memory mode: {}",
        config.general.memory_mode.as_str()
    );
    info_log!(
        INIT_LOGGER_DOMAIN,
        "Stream mode: {}",
        config.general.stream_mode
    );
    info_log!(INIT_LOGGER_DOMAIN, "User agent: {}", config.user_agent)
}

fn setup_load_config(
    run_args: &RunArgs,
) -> Result<LoadConfigOutcome, embystream::config::error::ConfigError> {
    match Config::load_or_init(run_args) {
        Ok(LoadConfigOutcome::Loaded(config)) => {
            info_log!(INIT_LOGGER_DOMAIN, "Configuration loaded successfully.");
            Ok(LoadConfigOutcome::Loaded(config))
        }
        Ok(LoadConfigOutcome::TemplateCreated(path)) => {
            Ok(LoadConfigOutcome::TemplateCreated(path))
        }
        Err(e) => Err(e),
    }
}

fn build_run_web_runtime_config(
    run_args: &RunArgs,
    config: Option<&Config>,
) -> Result<WebRuntimeConfig, embystream::web::error::WebError> {
    let runtime_log_dir = if run_args.web_runtime_log_dir.as_os_str().is_empty()
    {
        config
            .map(|config| PathBuf::from(&config.log.root_path))
            .unwrap_or_else(|| PathBuf::from("web-config/logs"))
    } else {
        run_args.web_runtime_log_dir.clone()
    };

    let stream_log_dir = config
        .map(|config| PathBuf::from(&config.log.root_path))
        .unwrap_or_else(|| PathBuf::from("./logs"));

    to_runtime_config(embystream::cli::WebServeArgs {
        listen: run_args.web_listen.clone(),
        data_dir: run_args.web_data_dir.clone(),
        tmdb_api_key: run_args.web_tmdb_api_key.clone(),
        runtime_log_dir,
        stream_log_dir,
    })
    .map(|mut runtime| {
        runtime.main_config_path = config.map(|config| config.path.clone());
        runtime
    })
}

fn start_web_service(config: WebRuntimeConfig) {
    tokio::spawn(async move {
        if let Err(error) = serve_web_app(config).await {
            eprintln!("Web studio failed: {}", error);
        }
    });
}

/// Environment variable that overrides whether the web configuration studio
/// starts alongside `run`. Recognized over both upper- and lower-case names.
const WEB_ENABLE_ENV_KEYS: &[&str] = &["WEB_ENABLE", "web_enable"];

/// Reports whether the web studio should start alongside `run`.
///
/// When `WEB_ENABLE` (or its lower-case alias) is set to a recognized boolean
/// value, it takes precedence over the `--web` flag. This lets deployments such
/// as the Docker image, which bakes `--web` into its default command, toggle
/// the studio purely through an environment variable. When the variable is
/// unset or holds an unrecognized value, the `--web` flag is used unchanged.
fn web_studio_enabled(run_args: &RunArgs) -> bool {
    WEB_ENABLE_ENV_KEYS
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .and_then(|value| parse_env_bool(&value))
        .unwrap_or(run_args.web)
}

/// Parses a boolean-like environment value, ignoring case and surrounding
/// whitespace. Returns `None` for unrecognized values so callers can fall back.
fn parse_env_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" | "enable" | "enabled" => Some(true),
        "0" | "false" | "no" | "off" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}

fn setup_logger(config: &Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let log_path = Path::new(&config.log.root_path);
    fs::create_dir_all(log_path)?;

    let level = LogLevel::from_str(&config.log.level).unwrap_or(LogLevel::Info);
    Logger::builder()
        .with_level(level)
        .with_directory(&config.log.root_path)
        .with_file_prefix(&config.log.prefix)
        .with_live_logs(global_log_stream(), "stream")
        .build();

    start_cleanup_task(config.log.root_path.clone());
    info_log!(
        INIT_LOGGER_DOMAIN,
        "Log cleanup task started (retention: 7 days)"
    );

    Ok(())
}

async fn setup_cache(config: &Config) -> Arc<AppState> {
    let app_state = AppState::new(config.clone()).await;

    let problematic_clients = app_state.get_problematic_clients().await;
    info_log!(
        INIT_LOGGER_DOMAIN,
        "Problematic clients: {:?}",
        problematic_clients
    );

    Arc::new(app_state)
}

fn validate_dual_mode_ports(config: &Config) -> Result<(), String> {
    if config.general.stream_mode == StreamMode::Dual {
        if let (Some(frontend), Some(backend)) =
            (&config.frontend, &config.backend)
        {
            if frontend.listen_port == backend.listen_port {
                return Err(format!(
                    "Dual mode port conflict: frontend & backend cannot both use {}.",
                    frontend.listen_port
                ));
            }
        }
    }
    Ok(())
}

fn setup_crypto_provider() -> Result<(), Box<dyn Error + Send + Sync>> {
    Gateway::setup_crypto_provider().map_err(|e| {
        error_log!(INIT_LOGGER_DOMAIN, "Setup crypto-provider failed: {:?}", e);
        e
    })
}

async fn setup_rate_limiters(app_state: &Arc<AppState>) {
    app_state.init_rate_limiters().await;
    info_log!(INIT_LOGGER_DOMAIN, "Rate limiter refill task started.");
}

async fn setup_google_drive_refresh(app_state: &Arc<AppState>) {
    let google_drive_node_count = {
        let config = app_state.get_config().await;
        config
            .backend_nodes
            .iter()
            .filter(|node| google_drive_auth::is_google_drive_node(node))
            .count()
    };

    if google_drive_node_count == 0 {
        debug_log!(
            INIT_LOGGER_DOMAIN,
            "Skipping googleDrive token prewarm/prerefresh tasks - no \
             googleDrive nodes"
        );
        return;
    }

    tokio::spawn(google_drive_auth::prewarm_google_drive_tokens(
        app_state.clone(),
    ));
    tokio::spawn(google_drive_auth::schedule_google_drive_token_refreshes(
        app_state.clone(),
    ));
    info_log!(
        INIT_LOGGER_DOMAIN,
        "googleDrive request-time token source enabled \
         (startup prewarm and expiry-driven prerefresh scheduled, nodes: {})",
        google_drive_node_count
    );
}

async fn setup_frontend_gateway(
    app_state: &Arc<AppState>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = app_state.get_config().await.clone();
    let mode = config.general.stream_mode;

    if !matches!(mode, StreamMode::Frontend | StreamMode::Dual) {
        debug_log!(
            INIT_LOGGER_DOMAIN,
            "Skipping frontend gateway setup - stream mode not enabled"
        );
        return Ok(());
    }

    debug_log!(INIT_LOGGER_DOMAIN, "Successfully start frontend listener");

    let frontend = config.frontend.as_ref().ok_or_else(|| {
        error_log!(
            INIT_LOGGER_DOMAIN,
            "Error: Frontend configuration not exist"
        );
        "Frontend config missing"
    })?;

    let addr = format!("0.0.0.0:{}", frontend.listen_port);
    let service = Arc::new(AppForwardService::new(app_state.clone()));

    let emby_base_url = config.emby.get_uri().to_string();

    info_log!(
        INIT_LOGGER_DOMAIN,
        "Frontend reverse proxy target: {}",
        emby_base_url
    );

    let mut gateway = Gateway::new(&addr)
        .add_middleware(Box::new(LoggerMiddleware))
        .add_middleware(Box::new(
            ClientAgentFilterMiddleware::new(app_state.clone())
                .with_filter_paths(COMPILED_UA_FILTERS.clone()),
        ))
        .add_middleware(Box::new(ReverseProxyFilterMiddleware::new(
            frontend.clone().anti_reverse_proxy,
        )))
        .add_middleware(Box::new(CorsMiddleware))
        .add_middleware(Box::new(OptionsMiddleware))
        .add_middleware(Box::new(PlaylistMockMiddleware))
        .add_middleware(Box::new(ForwardMiddleware::new(service)))
        .add_middleware(Box::new(ReverseProxyMiddleware::new(
            emby_base_url,
            app_state.clone(),
        )));

    gateway.set_handler(default_handler());
    gateway.listen().await?;

    Ok(())
}

async fn setup_backend_gateway(
    app_state: &Arc<AppState>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = app_state.get_config().await.clone();
    let mode = config.general.clone().stream_mode;

    if !matches!(mode, StreamMode::Backend | StreamMode::Dual) {
        debug_log!(
            INIT_LOGGER_DOMAIN,
            "Skipping backend gateway setup - stream mode not enabled"
        );
        return Ok(());
    }

    debug_log!(INIT_LOGGER_DOMAIN, "Successfully start backend listener");

    let backend = config.backend.as_ref().ok_or_else(|| {
        error_log!(
            INIT_LOGGER_DOMAIN,
            "Error: Backend configuration not exist"
        );
        "Backend config missing"
    })?;

    let addr = format!("0.0.0.0:{}", backend.listen_port);
    let service = Arc::new(AppStreamService::new(app_state.clone()));

    let mut gateway = Gateway::new(&addr)
        .with_tls(config.get_ssl_cert_path(), config.get_ssl_key_path())
        .add_middleware(Box::new(LoggerMiddleware))
        .add_middleware(Box::new(ClientAgentFilterMiddleware::new(
            app_state.clone(),
        )))
        .add_middleware(Box::new(CorsMiddleware))
        .add_middleware(Box::new(OptionsMiddleware))
        .add_middleware(Box::new(StreamRelayMiddleware::new(
            config.backend_nodes.clone(),
        )))
        .add_middleware(Box::new(StreamMiddleware::new(
            config.backend_nodes.clone(),
            service,
            app_state.clone(),
        )));

    gateway.set_handler(default_handler());
    gateway.listen().await?;

    Ok(())
}

fn default_handler() -> Handler {
    Arc::new(|_ctx: Context, _body: Option<Incoming>| {
        debug_log!(GATEWAY_LOGGER_DOMAIN, "Fallback to default middleware...");
        ResponseBuilder::with_status_code(StatusCode::SERVICE_UNAVAILABLE)
    })
}

#[cfg(test)]
mod web_enable_env_tests {
    use super::parse_env_bool;

    #[test]
    fn parses_truthy_values_case_and_space_insensitively() {
        for value in ["1", "true", "TRUE", " yes ", "On", "enable", "ENABLED"] {
            assert_eq!(parse_env_bool(value), Some(true), "value = {value:?}");
        }
    }

    #[test]
    fn parses_falsy_values_case_and_space_insensitively() {
        for value in
            ["0", "false", "FALSE", " no ", "Off", "disable", "DISABLED"]
        {
            assert_eq!(parse_env_bool(value), Some(false), "value = {value:?}");
        }
    }

    #[test]
    fn returns_none_for_unrecognized_values() {
        for value in ["", "maybe", "2", "web"] {
            assert_eq!(parse_env_bool(value), None, "value = {value:?}");
        }
    }
}
