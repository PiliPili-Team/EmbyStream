//! Interactive config wizard and `config show`.

use std::{env, fs, io::Write, path::Path};

use anyhow::{Result, anyhow};
use chrono::Utc;
use dialoguer::{Input, Select};
use rand::Rng;

use crate::i18n::{tr, tr_fmt};
use crate::{
    cli::ConfigArgs,
    config::{
        backend::{
            Backend, BackendNode, direct::DirectLink, disk::Disk,
            google_drive::GoogleDriveConfig, openlist::OpenList,
            webdav::WebDavConfig,
        },
        core::{finish_raw_config, parse_raw_config_str},
        frontend::Frontend,
        general::{Emby, General, Log, StreamMode, UserAgent},
        http2::Http2,
        types::{
            AntiReverseProxyConfig, FallbackConfig, PathRewriteConfig,
            RawConfig,
        },
    },
    core::backend::constants::STREAM_RELAY_BACKEND_TYPE,
    core::backend::webdav::BACKEND_TYPE,
};

use super::{
    discover::{DiscoveredConfig, discover_configs},
    emit::emit_wizard_config_toml,
    l10n::{auto_generated_display, empty_display, secret_masked_display},
    mask::mask_toml_secrets,
    persist::{path_exists, safe_join_cwd, write_atomic},
    regex_lab::{prompt_regex_until_ok, regex_playground, try_compile_regex},
    template_payload::build_template_raw,
    terminal::{
        WIZ_DIALOG_LINES_BELOW_QUESTION, print_error, print_field_input_tip,
        print_field_intro_line, print_field_result_separator,
        print_field_value_line, print_field_value_line_compact, print_hint,
        print_ok, print_select_file_list_tip, print_subsection_title,
        print_table_header, print_title, print_welcome_banner,
        print_yes_no_result, rewrite_default_prompt_as_checkmark,
    },
    wizard_input_theme::{WIZ_INPUT_THEME, WizardInputTheme},
};

fn theme() -> dialoguer::theme::ColorfulTheme {
    dialoguer::theme::ColorfulTheme::default()
}

fn input_theme() -> &'static WizardInputTheme {
    &WIZ_INPUT_THEME
}

/// `intro()` prints `===> field (…, Default: … / Example: …)`; no `===> ? …` preview —
/// use `wiz_input_*` after.
const WIZARD_INPUT_PROMPT: &str = "";

/// Yes/No via arrow-key selection (default option highlighted).
fn confirm_yes_no(prompt: impl AsRef<str>, default_yes: bool) -> Result<bool> {
    let prompt = prompt.as_ref();
    let yes_l = tr("wizard.yes");
    let no_l = tr("wizard.no");
    let items = [&yes_l, &no_l];
    let default = if default_yes { 0usize } else { 1 };
    print_field_intro_line(prompt, "", None, None);
    let i = Select::with_theme(&theme())
        .with_prompt("")
        .items(items)
        .default(default)
        .report(false)
        .interact()
        .map_err(|e| anyhow!(e.to_string()))?;
    let yes = i == 0;
    print_yes_no_result(if yes { yes_l.as_str() } else { no_l.as_str() });
    Ok(yes)
}

fn wiz_input_string(
    default: Option<String>,
    allow_empty: bool,
) -> Result<String> {
    let mut first_tip = true;
    loop {
        let previewed_default = default.is_some();
        if default.is_some() {
            print_field_input_tip();
        } else if first_tip {
            print_field_input_tip();
            first_tip = false;
        } else {
            print_field_input_tip();
        }
        let s: String = Input::with_theme(input_theme())
            .with_prompt(WIZARD_INPUT_PROMPT)
            .allow_empty(true)
            .report(false)
            .interact_text()
            .map_err(|e| anyhow!(e.to_string()))?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            if let Some(ref d) = default {
                let disp = if d.trim().is_empty() {
                    empty_display()
                } else {
                    d.trim().to_string()
                };
                rewrite_default_prompt_as_checkmark(
                    &disp,
                    WIZ_DIALOG_LINES_BELOW_QUESTION,
                    None,
                );
                return Ok(d.clone());
            }
            if allow_empty {
                print_field_value_line(empty_display());
                return Ok(String::new());
            }
            print_error(tr("wizard.error.value_required"));
            continue;
        }
        if previewed_default {
            rewrite_default_prompt_as_checkmark(
                trimmed,
                WIZ_DIALOG_LINES_BELOW_QUESTION,
                None,
            );
        } else {
            print_field_value_line(trimmed);
        }
        return Ok(trimmed.to_string());
    }
}

fn wiz_input_string_no_echo(
    default: Option<String>,
    allow_empty: bool,
) -> Result<String> {
    let mut first_tip = true;
    loop {
        if default.is_some() {
            print_field_input_tip();
        } else if first_tip {
            print_field_input_tip();
            first_tip = false;
        } else {
            print_field_input_tip();
        }
        let s: String = Input::with_theme(input_theme())
            .with_prompt(WIZARD_INPUT_PROMPT)
            .allow_empty(true)
            .report(false)
            .interact_text()
            .map_err(|e| anyhow!(e.to_string()))?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            if let Some(d) = default {
                return Ok(d);
            }
            if allow_empty {
                return Ok(String::new());
            }
            print_error(tr("wizard.error.value_required"));
            continue;
        }
        return Ok(trimmed.to_string());
    }
}

fn wiz_input_u16(default: u16) -> Result<u16> {
    loop {
        print_field_input_tip();
        let s: String = Input::with_theme(input_theme())
            .with_prompt(WIZARD_INPUT_PROMPT)
            .allow_empty(true)
            .report(false)
            .interact_text()
            .map_err(|e| anyhow!(e.to_string()))?;
        if s.trim().is_empty() {
            rewrite_default_prompt_as_checkmark(
                &default.to_string(),
                WIZ_DIALOG_LINES_BELOW_QUESTION,
                None,
            );
            return Ok(default);
        }
        match s.trim().parse::<u16>() {
            Ok(v) => {
                rewrite_default_prompt_as_checkmark(
                    &v.to_string(),
                    WIZ_DIALOG_LINES_BELOW_QUESTION,
                    None,
                );
                return Ok(v);
            }
            Err(_) => print_error(tr("wizard.error.port_range")),
        }
    }
}

fn wiz_input_i32(default: i32) -> Result<i32> {
    loop {
        print_field_input_tip();
        let s: String = Input::with_theme(input_theme())
            .with_prompt(WIZARD_INPUT_PROMPT)
            .allow_empty(true)
            .report(false)
            .interact_text()
            .map_err(|e| anyhow!(e.to_string()))?;
        if s.trim().is_empty() {
            rewrite_default_prompt_as_checkmark(
                &default.to_string(),
                WIZ_DIALOG_LINES_BELOW_QUESTION,
                None,
            );
            return Ok(default);
        }
        match s.trim().parse::<i32>() {
            Ok(v) => {
                rewrite_default_prompt_as_checkmark(
                    &v.to_string(),
                    WIZ_DIALOG_LINES_BELOW_QUESTION,
                    None,
                );
                return Ok(v);
            }
            Err(_) => print_error(tr("wizard.error.integer_invalid")),
        }
    }
}

fn wiz_input_u64(default: u64) -> Result<u64> {
    loop {
        print_field_input_tip();
        let s: String = Input::with_theme(input_theme())
            .with_prompt(WIZARD_INPUT_PROMPT)
            .allow_empty(true)
            .report(false)
            .interact_text()
            .map_err(|e| anyhow!(e.to_string()))?;
        if s.trim().is_empty() {
            rewrite_default_prompt_as_checkmark(
                &default.to_string(),
                WIZ_DIALOG_LINES_BELOW_QUESTION,
                None,
            );
            return Ok(default);
        }
        match s.trim().parse::<u64>() {
            Ok(v) => {
                rewrite_default_prompt_as_checkmark(
                    &v.to_string(),
                    WIZ_DIALOG_LINES_BELOW_QUESTION,
                    None,
                );
                return Ok(v);
            }
            Err(_) => print_error(tr("wizard.error.non_negative_integer")),
        }
    }
}

/// Entry from `main`.
pub fn run(args: &ConfigArgs) -> Result<()> {
    let cwd = env::current_dir()?;
    match args.sub {
        None => run_main_menu(&cwd),
        Some(crate::cli::ConfigSubcommand::Show) => run_show_flow(&cwd),
        Some(crate::cli::ConfigSubcommand::Template) => run_template_flow(&cwd),
    }
}

fn run_show_flow(cwd: &Path) -> Result<()> {
    let list = discover_configs(cwd)?;
    print_discovered_table(&list);
    if list.is_empty() {
        print_hint(tr("wizard.error.no_valid_toml_here"));
        return Ok(());
    }
    print_field_intro_line(
        tr("wizard.menu.select_file_display"),
        "",
        None,
        None,
    );
    print_select_file_list_tip();
    let idx = Select::with_theme(&theme())
        .with_prompt("")
        .items(
            list.iter()
                .map(|d| {
                    format!(
                        "{}  (stream_mode={})",
                        d.path.display(),
                        d.stream_mode
                    )
                })
                .collect::<Vec<_>>(),
        )
        .default(0)
        .report(false)
        .interact_opt()
        .map_err(|e| anyhow!(e.to_string()))?;
    let Some(idx) = idx else {
        return Ok(());
    };
    let content = fs::read_to_string(&list[idx].path)?;
    let mut masked = true;
    if confirm_yes_no(tr("wizard.confirm.show_secrets_plain"), false)? {
        masked = false;
    }
    print_field_result_separator();
    if masked {
        print_title(tr("wizard.section.masked_content"));
        print!("{}", mask_toml_secrets(&content));
    } else {
        print_title(tr("wizard.section.file_content"));
        print!("{content}");
    }
    std::io::stdout().flush()?;
    Ok(())
}

fn default_template_filename(mode: StreamMode) -> String {
    let ts = Utc::now().format("%Y%m%d_%H%M%S");
    format!("{ts}_{mode}_template.toml")
}

fn run_template_flow(cwd: &Path) -> Result<()> {
    print_title(tr("wizard.section.configuration_template"));
    print_hint(tr("wizard.prompt.new_config_stream_mode"));
    let mode = select_stream_mode()?;
    let default_name = default_template_filename(mode);
    let fname: String = {
        print_field_intro_line(
            tr("wizard.field.file_name"),
            tr("wizard.prompt.write_under_cwd"),
            Some(default_name.as_str()),
            None,
        );
        print_field_input_tip();
        let s: String = Input::with_theme(input_theme())
            .with_prompt(WIZARD_INPUT_PROMPT)
            .allow_empty(true)
            .report(false)
            .interact_text()
            .map_err(|e| anyhow!(e.to_string()))?;
        if s.trim().is_empty() {
            default_name
        } else {
            s.trim().to_string()
        }
    };
    rewrite_default_prompt_as_checkmark(
        &fname,
        WIZ_DIALOG_LINES_BELOW_QUESTION,
        None,
    );
    let dest = safe_join_cwd(cwd, fname.trim())
        .ok_or_else(|| anyhow!("invalid file name"))?;
    if path_exists(&dest) {
        print_error(tr("wizard.error.file_exists"));
        return Ok(());
    }
    let raw = build_template_raw(mode);
    finish_raw_config(dest.clone(), raw.clone()).map_err(|e| anyhow!("{e}"))?;
    let toml = emit_wizard_config_toml(&raw)?;
    write_atomic(&dest, &toml).map_err(|e| anyhow!("{e}"))?;
    print_ok(format!(
        "{}{}",
        tr("wizard.msg.prefix.wrote_template"),
        dest.display()
    ));
    Ok(())
}

fn run_main_menu(cwd: &Path) -> Result<()> {
    let mut first_menu = true;
    loop {
        if first_menu {
            print_welcome_banner();
            first_menu = false;
        }
        let list = discover_configs(cwd)?;
        print_discovered_table(&list);
        let items = vec![
            tr("wizard.menu.new_config_file"),
            tr("wizard.menu.edit_existing"),
            tr("wizard.menu.delete"),
            tr("wizard.menu.rename"),
            tr("wizard.menu.copy"),
            tr("wizard.menu.quit"),
        ];
        print_field_intro_line(
            tr("wizard.menu.main"),
            tr("wizard.prompt.pick_action_arrows"),
            None,
            None,
        );
        let sel = Select::with_theme(&theme())
            .with_prompt("")
            .items(&items)
            .default(0)
            .report(false)
            .interact()
            .map_err(|e| anyhow!(e.to_string()))?;
        match sel {
            0 => run_new_flow(cwd)?,
            1 => {
                if list.is_empty() {
                    print_error(tr("wizard.error.no_configs_to_edit"));
                    continue;
                }
                let p_edit = tr("wizard.menu.select_file_edit");
                let Some(i) = pick_discovered(&list, &p_edit)? else {
                    continue;
                };
                let path = list[i].path.clone();
                let raw = parse_raw_config_str(&fs::read_to_string(&path)?)?;
                let mut updated = run_edit_loop(raw)?;
                save_config_file(&path, &mut updated)?;
                print_ok(tr("wizard.msg.status.saved"));
            }
            2 => {
                if list.is_empty() {
                    print_error(tr("wizard.error.no_configs_to_delete"));
                    continue;
                }
                let p_del = tr("wizard.menu.select_file_delete");
                let Some(i) = pick_discovered(&list, &p_del)? else {
                    continue;
                };
                let p = &list[i].path;
                let del_msg = tr_fmt(
                    "wizard.confirm.permanent_delete",
                    &[("path", &p.display().to_string())],
                );
                if confirm_yes_no(&del_msg, false)? {
                    fs::remove_file(p)?;
                    print_ok(tr("wizard.msg.status.deleted"));
                }
            }
            3 => {
                if list.is_empty() {
                    print_error(tr("wizard.error.no_configs_to_rename"));
                    continue;
                }
                let p_ren = tr("wizard.menu.select_file_rename");
                let Some(i) = pick_discovered(&list, &p_ren)? else {
                    continue;
                };
                print_field_input_tip();
                let new_name: String = Input::with_theme(input_theme())
                    .with_prompt(tr("wizard.prompt.new_file_name_example"))
                    .report(false)
                    .interact_text()
                    .map_err(|e| anyhow!(e.to_string()))?;
                let dest = safe_join_cwd(cwd, &new_name)
                    .ok_or_else(|| anyhow!("invalid file name"))?;
                if path_exists(&dest) {
                    print_error(tr("wizard.error.target_exists"));
                    continue;
                }
                fs::rename(&list[i].path, &dest)?;
                print_ok(tr("wizard.msg.status.renamed"));
            }
            4 => {
                if list.is_empty() {
                    print_error(tr("wizard.error.no_configs_to_copy"));
                    continue;
                }
                let p_cp = tr("wizard.menu.select_file_copy");
                let Some(i) = pick_discovered(&list, &p_cp)? else {
                    continue;
                };
                print_field_input_tip();
                let new_name: String = Input::with_theme(input_theme())
                    .with_prompt(tr("wizard.prompt.new_file_name"))
                    .report(false)
                    .interact_text()
                    .map_err(|e| anyhow!(e.to_string()))?;
                let dest = safe_join_cwd(cwd, &new_name)
                    .ok_or_else(|| anyhow!("invalid file name"))?;
                if path_exists(&dest) {
                    print_error(tr("wizard.error.target_exists"));
                    continue;
                }
                fs::copy(&list[i].path, &dest)?;
                print_ok(tr("wizard.msg.status.copied"));
            }
            5 => break,
            _ => break,
        }
    }
    Ok(())
}

fn pick_discovered(
    list: &[DiscoveredConfig],
    prompt: impl AsRef<str>,
) -> Result<Option<usize>> {
    print_field_intro_line(prompt.as_ref(), "", None, None);
    print_select_file_list_tip();
    Select::with_theme(&theme())
        .with_prompt("")
        .items(
            list.iter()
                .map(|d| format!("{}  ({})", d.path.display(), d.stream_mode))
                .collect::<Vec<_>>(),
        )
        .default(0)
        .report(false)
        .interact_opt()
        .map_err(|e| anyhow!(e.to_string()))
}

fn print_discovered_table(list: &[DiscoveredConfig]) {
    print_title(tr("wizard.section.configs_in_cwd"));
    if list.is_empty() {
        print_hint(tr("wizard.placeholder.paren_none"));
        print_field_result_separator();
        return;
    }
    print_table_header(
        tr("wizard.field.idx"),
        tr("wizard.field.file"),
        tr("wizard.field.stream_mode"),
    );
    for (i, d) in list.iter().enumerate() {
        let name = d.path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
        println!("  {:<4}  {:<38}  {}", i, name, d.stream_mode);
    }
    print_field_result_separator();
}

fn run_new_flow(cwd: &Path) -> Result<()> {
    let mode = select_stream_mode()?;
    let default_name = default_filename(mode);
    let fname: String = {
        print_field_intro_line(
            tr("wizard.field.file_name"),
            tr("wizard.prompt.write_under_cwd"),
            Some(default_name.as_str()),
            None,
        );
        print_field_input_tip();
        let s: String = Input::with_theme(input_theme())
            .with_prompt(WIZARD_INPUT_PROMPT)
            .allow_empty(true)
            .report(false)
            .interact_text()
            .map_err(|e| anyhow!(e.to_string()))?;
        if s.trim().is_empty() {
            default_name
        } else {
            s.trim().to_string()
        }
    };
    rewrite_default_prompt_as_checkmark(
        &fname,
        WIZ_DIALOG_LINES_BELOW_QUESTION,
        None,
    );
    let dest = safe_join_cwd(cwd, fname.trim())
        .ok_or_else(|| anyhow!("invalid file name"))?;
    if path_exists(&dest) {
        print_error(tr("wizard.error.file_exists"));
        return Ok(());
    }

    let mut raw = build_new_raw_skeleton(mode)?;
    prompt_shared_sections(&mut raw)?;
    match mode {
        StreamMode::Frontend => {
            raw.frontend = Some(prompt_frontend_section()?);
        }
        StreamMode::Backend => {
            raw.backend = Some(prompt_backend_section()?);
            raw.backend_nodes = Some(prompt_backend_nodes_loop()?);
        }
        StreamMode::Dual => {
            raw.frontend = Some(prompt_frontend_section()?);
            raw.backend = Some(prompt_backend_section()?);
            resolve_dual_listen_ports(&mut raw)?;
            raw.backend_nodes = Some(prompt_backend_nodes_loop()?);
        }
    }

    validate_and_preview(&mut raw, &dest)?;
    if !confirm_yes_no(tr("wizard.confirm.write_config_disk"), true)? {
        print_hint(tr("wizard.msg.status.discarded"));
        return Ok(());
    }
    save_config_file(&dest, &mut raw)?;
    print_ok(format!(
        "{}{}",
        tr("wizard.msg.prefix.wrote"),
        dest.display()
    ));

    if confirm_yes_no(tr("wizard.confirm.create_another_config"), false)? {
        run_new_flow(cwd)?;
    }
    Ok(())
}

/// If dual mode ports collide, prompt until they differ.
fn resolve_dual_listen_ports(raw: &mut RawConfig) -> Result<()> {
    if raw.general.stream_mode != StreamMode::Dual {
        return Ok(());
    }
    loop {
        let (Some(fe), Some(be)) = (&raw.frontend, &raw.backend) else {
            return Ok(());
        };
        if fe.listen_port != be.listen_port {
            return Ok(());
        }
        print_error(tr_fmt(
            "wizard.error.dual_port",
            &[("port", &fe.listen_port.to_string())],
        ));
        let a = tr("wizard.menu.change_frontend_listen_port");
        let b = tr("wizard.menu.change_backend_listen_port");
        let items = [&a, &b];
        print_field_intro_line(
            tr("wizard.prompt.which_port_to_change"),
            tr("wizard.prompt.pick_frontend_or_backend_port"),
            None,
            None,
        );
        let sel = Select::with_theme(&theme())
            .with_prompt("")
            .items(items)
            .default(0)
            .report(false)
            .interact()
            .map_err(|e| anyhow!(e.to_string()))?;
        match sel {
            0 => {
                if let Some(fe_mut) = raw.frontend.as_mut() {
                    let cur = fe_mut.listen_port;
                    let cur_s = cur.to_string();
                    intro(
                        tr("wizard.field.listen_port"),
                        tr("wizard.error.dual_must_differ_backend_port"),
                        Some(cur_s.as_str()),
                        None,
                    );
                    fe_mut.listen_port = wiz_input_u16(cur)?;
                }
            }
            _ => {
                if let Some(be_mut) = raw.backend.as_mut() {
                    let cur = be_mut.listen_port;
                    let cur_s = cur.to_string();
                    intro(
                        tr("wizard.field.listen_port"),
                        tr("wizard.error.dual_must_differ_frontend_port"),
                        Some(cur_s.as_str()),
                        None,
                    );
                    be_mut.listen_port = wiz_input_u16(cur)?;
                }
            }
        }
    }
}

fn default_filename(mode: StreamMode) -> String {
    let ts = Utc::now().format("%Y%m%d_%H%M%S");
    match mode {
        StreamMode::Frontend => format!("{ts}_frontend.toml"),
        StreamMode::Backend => format!("{ts}_backend.toml"),
        StreamMode::Dual => format!("{ts}_dual.toml"),
    }
}

fn select_stream_mode() -> Result<StreamMode> {
    print_title(tr("wizard.section.stream_mode"));
    print_hint(tr("wizard.hint.stream_mode.frontend"));
    print_hint(tr("wizard.hint.stream_mode.backend"));
    print_hint(tr("wizard.hint.stream_mode.dual"));
    print_field_intro_line(
        tr("wizard.field.stream_mode"),
        tr("wizard.prompt.choose_stream_mode_three"),
        None,
        None,
    );
    let items = vec![
        tr("wizard.option.stream_mode.frontend"),
        tr("wizard.option.stream_mode.backend"),
        tr("wizard.option.stream_mode.dual"),
    ];
    let i = Select::with_theme(&theme())
        .with_prompt("")
        .items(&items)
        .default(0)
        .report(false)
        .interact()
        .map_err(|e| anyhow!(e.to_string()))?;
    let mode = match i {
        1 => StreamMode::Backend,
        2 => StreamMode::Dual,
        _ => StreamMode::Frontend,
    };
    let mode_label = match mode {
        StreamMode::Frontend => tr("wizard.option.stream_mode.frontend"),
        StreamMode::Backend => tr("wizard.option.stream_mode.backend"),
        StreamMode::Dual => tr("wizard.option.stream_mode.dual"),
    };
    print_field_value_line(&mode_label);
    Ok(mode)
}

fn build_new_raw_skeleton(mode: StreamMode) -> Result<RawConfig> {
    Ok(RawConfig {
        general: General {
            memory_mode: "middle".into(),
            stream_mode: mode,
            encipher_key: String::new(),
            encipher_iv: String::new(),
        },
        log: Log {
            level: "info".into(),
            prefix: String::new(),
            root_path: "./logs".into(),
        },
        emby: Emby {
            url: tr("wizard.example.url.local_emby"),
            port: "8096".into(),
            token: String::new(),
        },
        user_agent: UserAgent {
            mode: "allow".into(),
            allow_ua: vec![],
            deny_ua: vec![],
        },
        http2: None,
        frontend: None,
        backend: None,
        backend_nodes: None,
        disk: None,
        open_list: None,
        direct_link: None,
        fallback: FallbackConfig::default(),
    })
}

fn intro(
    field: impl AsRef<str>,
    purpose: impl AsRef<str>,
    default_hint: Option<&str>,
    example: Option<&str>,
) {
    print_field_intro_line(field, purpose, default_hint, example);
}

fn input_text_w_echo(
    default: Option<String>,
    allow_empty: bool,
) -> Result<String> {
    wiz_input_string(default, allow_empty)
}

fn input_secret_w_echo(allow_empty: bool) -> Result<String> {
    print_field_input_tip();
    let s: String = Input::with_theme(input_theme())
        .with_prompt(WIZARD_INPUT_PROMPT)
        .allow_empty(allow_empty)
        .report(false)
        .interact_text()
        .map_err(|e| anyhow!(e.to_string()))?;
    let disp = if s.trim().is_empty() {
        empty_display()
    } else {
        secret_masked_display()
    };
    print_field_value_line(&disp);
    Ok(s)
}

fn prompt_shared_sections(raw: &mut RawConfig) -> Result<()> {
    print_title(tr("wizard.section.log"));
    let def_level = raw.log.level.clone();
    intro(
        tr("wizard.field.level"),
        tr("wizard.prompt.log_level_tracing"),
        Some(def_level.as_str()),
        None,
    );
    raw.log.level = wiz_input_string(Some(def_level), false)?;
    let def_root = raw.log.root_path.clone();
    let root_disp = if def_root.trim().is_empty() {
        empty_display()
    } else {
        def_root.trim().to_string()
    };
    intro(
        tr("wizard.field.root_path"),
        tr("wizard.prompt.log_root_path"),
        Some(root_disp.as_str()),
        None,
    );
    raw.log.root_path = wiz_input_string(Some(def_root), false)?;
    let def_prefix = raw.log.prefix.clone();
    let prefix_disp = if def_prefix.trim().is_empty() {
        empty_display()
    } else {
        def_prefix.trim().to_string()
    };
    intro(
        tr("wizard.field.prefix"),
        tr("wizard.prompt.log_prefix_optional"),
        Some(prefix_disp.as_str()),
        None,
    );
    raw.log.prefix = wiz_input_string(Some(def_prefix), true)?;

    print_title(tr("wizard.section.general"));
    raw.general.memory_mode = prompt_memory_mode(&raw.general.memory_mode)?;
    intro(
        tr("wizard.field.encipher_key"),
        tr("wizard.prompt.encipher_key_aes"),
        None,
        Some(tr("wizard.hint.press_enter_auto_generate").as_str()),
    );
    let key_in: String = wiz_input_string_no_echo(None, true)?;
    raw.general.encipher_key = if key_in.trim().is_empty() {
        let v = random_alnum(16);
        print_field_value_line(auto_generated_display());
        v
    } else {
        print_field_value_line(key_in.trim());
        key_in
    };

    intro(
        tr("wizard.field.encipher_iv"),
        tr("wizard.prompt.encipher_iv_aes"),
        None,
        Some(tr("wizard.hint.press_enter_auto_generate").as_str()),
    );
    let iv_in: String = wiz_input_string_no_echo(None::<String>, true)?;
    raw.general.encipher_iv = if iv_in.trim().is_empty() {
        let v = random_alnum(16);
        print_field_value_line(auto_generated_display());
        v
    } else {
        print_field_value_line(iv_in.trim());
        iv_in
    };

    print_title(tr("wizard.section.emby"));
    let def_url = raw.emby.url.clone();
    let url_disp = if def_url.trim().is_empty() {
        empty_display()
    } else {
        def_url.trim().to_string()
    };
    intro(
        tr("wizard.field.url"),
        tr("wizard.prompt.emby_base_url"),
        Some(url_disp.as_str()),
        None,
    );
    let url_in: String = wiz_input_string_no_echo(Some(def_url), true)?;
    raw.emby.url = normalize_emby_url(&url_in);
    rewrite_default_prompt_as_checkmark(
        raw.emby.url.as_str(),
        WIZ_DIALOG_LINES_BELOW_QUESTION,
        None,
    );
    let def_emby_port = raw.emby.port.clone();
    intro(
        tr("wizard.field.port"),
        tr("wizard.prompt.emby_http_port"),
        Some(def_emby_port.as_str()),
        None,
    );
    raw.emby.port = wiz_input_string(Some(def_emby_port), false)?;
    intro(
        tr("wizard.field.token"),
        tr("wizard.prompt.emby_api_token"),
        None,
        Some(tr("wizard.example.token.paste_here").as_str()),
    );
    raw.emby.token = wiz_input_string(None, false)?;

    print_title(tr("wizard.section.user_agent"));
    raw.user_agent.mode = prompt_user_agent_mode(&raw.user_agent.mode)?;
    raw.user_agent.allow_ua = prompt_ua_token_list(
        tr("wizard.field.allow_ua"),
        tr("wizard.prompt.user_agent_allow_tokens"),
        false,
    )?;
    raw.user_agent.deny_ua = prompt_ua_token_list(
        tr("wizard.field.deny_ua"),
        tr("wizard.prompt.user_agent_deny_tokens"),
        true,
    )?;

    print_title(tr("wizard.section.http2"));
    intro(
        tr("wizard.field.ssl_cert_file"),
        tr("wizard.prompt.ssl_cert_pem_path"),
        None,
        None,
    );
    let cert: String = wiz_input_string(None, true)?;
    intro(
        tr("wizard.field.ssl_key_file"),
        tr("wizard.prompt.ssl_private_key_pem_path"),
        None,
        None,
    );
    let key: String = wiz_input_string(None, true)?;
    if !cert.is_empty() || !key.is_empty() {
        raw.http2 = Some(Http2 {
            ssl_cert_file: cert,
            ssl_key_file: key,
        });
    }

    print_title(tr("wizard.section.fallback"));
    intro(
        tr("wizard.field.video_missing_path"),
        tr("wizard.prompt.video_missing_local_file"),
        None,
        Some(tr("wizard.example.file.fallback_mp4").as_str()),
    );
    raw.fallback.video_missing_path = wiz_input_string(None, true)?;

    Ok(())
}

fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn normalize_emby_url(raw_input: &str) -> String {
    let t = raw_input.trim();
    if t.is_empty() {
        return tr("wizard.example.url.local_emby");
    }
    if t.contains("://") {
        t.to_string()
    } else {
        format!("http://{t}")
    }
}

fn prompt_user_agent_mode(current: &str) -> Result<String> {
    const VALUES: &[&str] = &["allow", "deny"];
    let t = current.trim();
    let def_disp = VALUES
        .iter()
        .copied()
        .find(|v| v.eq_ignore_ascii_case(t))
        .unwrap_or("allow");
    let labels = vec![
        tr("wizard.option.ua_mode.allow"),
        tr("wizard.option.ua_mode.deny"),
    ];
    intro(
        tr("wizard.field.mode"),
        tr("wizard.prompt.ua_mode_explainer"),
        Some(def_disp),
        None,
    );
    let idx = VALUES
        .iter()
        .position(|v| v.eq_ignore_ascii_case(t))
        .unwrap_or(0);
    let i = Select::with_theme(&theme())
        .with_prompt("")
        .items(&labels)
        .default(idx)
        .report(false)
        .interact()
        .map_err(|e| anyhow!(e.to_string()))?;
    let v = VALUES[i].to_string();
    print_field_value_line(&v);
    Ok(v)
}

fn prompt_memory_mode(current: &str) -> Result<String> {
    intro(
        tr("wizard.field.memory_mode"),
        tr("wizard.prompt.memory_mode_explainer"),
        Some(current),
        None,
    );
    const VALUES: &[&str] = &["low", "middle", "high"];
    let labels = vec![
        tr("wizard.option.memory.low"),
        tr("wizard.option.memory.middle"),
        tr("wizard.option.memory.high"),
    ];
    if let Some(idx) = VALUES.iter().position(|&v| v == current) {
        let i = Select::with_theme(&theme())
            .with_prompt("")
            .items(&labels)
            .default(idx)
            .report(false)
            .interact()
            .map_err(|e| anyhow!(e.to_string()))?;
        let v = VALUES[i].to_string();
        print_field_value_line(&v);
        return Ok(v);
    }
    let s = wiz_input_string(Some(current.to_string()), false)?;
    Ok(s)
}

/// `skip_leading_input_tip`: set true for the second of back-to-back UA lists (`deny_ua` after `allow_ua`)
/// so the same `Tip:` line is not printed twice in a row.
fn prompt_ua_token_list(
    field_label: impl AsRef<str>,
    purpose: impl AsRef<str>,
    skip_leading_input_tip: bool,
) -> Result<Vec<String>> {
    intro(
        field_label,
        purpose,
        None,
        Some(tr("wizard.example.user_agent.mozilla").as_str()),
    );
    if !skip_leading_input_tip {
        print_field_input_tip();
    }
    print_hint(tr("wizard.prompt.ua_token_one_per_line"));
    let mut out = Vec::new();
    loop {
        let line: String = Input::with_theme(input_theme())
            .with_prompt(WIZARD_INPUT_PROMPT)
            .allow_empty(true)
            .report(false)
            .interact_text()
            .map_err(|e| anyhow!(e.to_string()))?;
        let t = line.trim();
        if t.is_empty() {
            break;
        }
        print_field_value_line_compact(t);
        out.push(t.to_string());
    }
    print_field_input_tip();
    print_field_result_separator();
    Ok(out)
}

fn random_alnum(len: usize) -> String {
    const CHARS: &[u8] =
        b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

fn prompt_frontend_section() -> Result<Frontend> {
    print_title(tr("wizard.section.frontend"));
    intro(
        tr("wizard.field.listen_port"),
        tr("wizard.prompt.frontend_listen_port_tcp"),
        Some("60001"),
        None,
    );
    let listen_port: u16 = wiz_input_u16(60001)?;
    let path_rewrites = prompt_path_rewrites(
        tr("wizard.label.path_rewrite"),
        tr("wizard.prompt.path_rewrite_to_emby_cdn"),
    )?;
    let anti = prompt_anti_reverse(tr("wizard.label.anti_reverse_proxy"))?;
    Ok(Frontend {
        listen_port,
        check_file_existence: false,
        path_rewrites,
        anti_reverse_proxy: anti,
    })
}

fn prompt_backend_section() -> Result<Backend> {
    print_title(tr("wizard.section.backend"));
    intro(
        tr("wizard.field.listen_port"),
        tr("wizard.prompt.backend_listen_port_tcp"),
        Some("60001"),
        None,
    );
    let listen_port: u16 = wiz_input_u16(60001)?;
    intro(
        tr("wizard.field.base_url"),
        tr("wizard.prompt.backend_public_base_url"),
        None,
        Some(tr("wizard.example.url.stream_https").as_str()),
    );
    let base_url: String = wiz_input_string(None, false)?;
    intro(
        tr("wizard.field.port"),
        tr("wizard.prompt.published_url_port"),
        Some("443"),
        None,
    );
    let port: String = wiz_input_string(Some("443".into()), false)?;
    intro(
        tr("wizard.field.path"),
        tr("wizard.prompt.stream_url_path_prefix"),
        None,
        Some("stream"),
    );
    let path: String = wiz_input_string(None, true)?;
    intro(
        tr("wizard.field.problematic_clients"),
        tr("wizard.prompt.problematic_clients_csv"),
        Some(tr("wizard.example.problematic_clients").as_str()),
        None,
    );
    let pc: String =
        wiz_input_string(Some(tr("wizard.example.problematic_clients")), true)?;
    let problematic_clients = split_csv(&pc);
    Ok(Backend {
        listen_port,
        base_url,
        port,
        path,
        check_file_existence: true,
        problematic_clients,
    })
}

fn prompt_anti_reverse(ctx: impl AsRef<str>) -> Result<AntiReverseProxyConfig> {
    let ctx = ctx.as_ref();
    intro(
        ctx,
        tr("wizard.prompt.anti_reverse_reject_bad_host"),
        None,
        Some(tr("wizard.example.toml.host_line").as_str()),
    );
    let enable_msg = tr_fmt("wizard.prompt.enable_anti", &[("ctx", ctx)]);
    let enable = confirm_yes_no(&enable_msg, false)?;
    let hosts: Vec<String> = if enable {
        intro(
            tr("wizard.field.host"),
            tr("wizard.prompt.trusted_host_header"),
            None,
            Some(tr("wizard.example.host.stream_example").as_str()),
        );
        let raw: String = wiz_input_string_no_echo(None, false)?;
        // Accept multiple hosts separated by comma, whitespace or newline.
        let parsed: Vec<String> = raw
            .split([',', ' ', '\n', '\t'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let disp = if parsed.is_empty() {
            empty_display()
        } else {
            parsed.join(", ")
        };
        print_field_value_line(&disp);
        parsed
    } else {
        Vec::new()
    };
    Ok(AntiReverseProxyConfig {
        enable,
        trusted_hosts: hosts,
    })
}

fn prompt_path_rewrites(
    ctx: impl AsRef<str>,
    purpose: impl AsRef<str>,
) -> Result<Vec<PathRewriteConfig>> {
    let ctx = ctx.as_ref();
    let purpose = purpose.as_ref();
    let mut out = vec![];
    print_field_intro_line(ctx, purpose, None, None);
    while confirm_yes_no(tr("wizard.confirm.add_path_rewrite_entry"), false)? {
        let enable = confirm_yes_no(
            tr("wizard.confirm.enable_path_rewrite_rule"),
            false,
        )?;
        intro(
            tr("wizard.field.pattern"),
            tr("wizard.prompt.path_rewrite_rust_regex"),
            None,
            Some(tr("wizard.example.regex.path_rewrite_media").as_str()),
        );
        let pattern: String = if enable {
            let Some(p) = prompt_regex_until_ok()? else {
                print_error(tr("wizard.msg.skipped_empty_pattern"));
                continue;
            };
            if confirm_yes_no(
                tr("wizard.confirm.regex_playground_for_pattern"),
                true,
            )? {
                if let Some(re) = try_compile_regex(&p) {
                    regex_playground(&re)?;
                }
            }
            p
        } else {
            let p: String = wiz_input_string_no_echo(None, true)?;
            let disp = if p.trim().is_empty() {
                empty_display()
            } else {
                p.trim().to_string()
            };
            print_field_value_line(&disp);
            p
        };
        intro(
            tr("wizard.field.replacement"),
            tr("wizard.prompt.replacement_capture_groups"),
            None,
            Some(tr("wizard.example.regex_group_1").as_str()),
        );
        let replacement: String = wiz_input_string(None, true)?;
        out.push(PathRewriteConfig {
            enable,
            pattern,
            replacement,
        });
    }
    Ok(out)
}

fn backend_type_labels() -> Vec<String> {
    vec![
        tr("wizard.option.backend_type.disk"),
        tr("wizard.option.backend_type.openlist"),
        tr("wizard.option.backend_type.direct_link"),
        tr("wizard.option.backend_type.google_drive"),
        tr("wizard.option.backend_type.webdav_long"),
        tr("wizard.option.backend_type.stream_relay"),
    ]
}

fn prompt_backend_nodes_loop() -> Result<Vec<BackendNode>> {
    let mut nodes = vec![];
    loop {
        print_title(tr("wizard.section.backend_node"));
        let prompt = if nodes.is_empty() {
            tr("wizard.confirm.add_backend_node")
        } else {
            tr("wizard.confirm.add_another_backend_node")
        };
        let default_first = nodes.is_empty();
        if !confirm_yes_no(prompt, default_first)? {
            break;
        }
        nodes.push(prompt_one_backend_node()?);
    }
    Ok(nodes)
}

fn prompt_one_backend_node() -> Result<BackendNode> {
    intro(
        tr("wizard.field.name"),
        tr("wizard.prompt.node_log_label"),
        None,
        Some(tr("wizard.example.name.my_openlist").as_str()),
    );
    let name: String = wiz_input_string(None, false)?;

    intro(
        tr("wizard.field.type"),
        tr("wizard.prompt.node_storage_kind"),
        None,
        None,
    );
    let labels = backend_type_labels();
    let tidx = Select::with_theme(&theme())
        .with_prompt("")
        .items(&labels)
        .default(0)
        .report(false)
        .interact()
        .map_err(|e| anyhow!(e.to_string()))?;

    let backend_type = match tidx {
        0 => tr("wizard.value.backend_type.disk"),
        1 => tr("wizard.value.backend_type.openlist"),
        2 => tr("wizard.value.backend_type.direct_link"),
        3 => tr("wizard.value.backend_type.google_drive"),
        4 => BACKEND_TYPE.to_string(),
        5 => STREAM_RELAY_BACKEND_TYPE.to_string(),
        _ => tr("wizard.value.backend_type.disk"),
    };
    print_field_value_line(&backend_type);

    intro(
        tr("wizard.field.pattern"),
        tr("wizard.prompt.node_path_regex"),
        None,
        Some(tr("wizard.example.regex.openlist_path").as_str()),
    );
    let pattern: String = wiz_input_string(None, true)?;
    if !pattern.is_empty() {
        if try_compile_regex(&pattern).is_none() {
            return Err(anyhow!(tr("wizard.error.regex_pattern_invalid")));
        }
        if confirm_yes_no(tr("wizard.confirm.open_regex_playground"), true)? {
            if let Some(re) = try_compile_regex(&pattern) {
                regex_playground(&re)?;
            }
        }
    }

    intro(
        tr("wizard.field.base_url"),
        tr("wizard.prompt.node_upstream_origin"),
        None,
        Some(tr("wizard.example.url.local_emby").as_str()),
    );
    let base_url: String = wiz_input_string(None, true)?;
    intro(
        tr("wizard.field.port"),
        tr("wizard.prompt.node_upstream_port"),
        None,
        Some("5244"),
    );
    let port: String = wiz_input_string(None, true)?;
    intro(
        tr("wizard.field.path"),
        tr("wizard.prompt.node_path_append"),
        None,
        Some(tr("wizard.example.path.openlist_segment").as_str()),
    );
    let path: String = wiz_input_string(None, true)?;

    intro(
        tr("wizard.field.proxy_mode"),
        tr("wizard.prompt.proxy_mode_long"),
        None,
        None,
    );
    let proxy_items = vec![
        tr("wizard.option.proxy_mode.redirect_long"),
        tr("wizard.option.proxy_mode.proxy_long"),
    ];
    let proxy_items = if backend_type.eq_ignore_ascii_case(BACKEND_TYPE) {
        vec![
            tr("wizard.option.proxy_mode.redirect_long"),
            tr("wizard.option.proxy_mode.proxy_long"),
            tr("wizard.option.proxy_mode.accel_redirect_long"),
        ]
    } else {
        proxy_items
    };
    let pidx = Select::with_theme(&theme())
        .with_prompt("")
        .items(&proxy_items)
        .default(0)
        .report(false)
        .interact()
        .map_err(|e| anyhow!(e.to_string()))?;
    let proxy_mode = match pidx {
        1 => tr("wizard.value.proxy_mode.proxy"),
        2 if backend_type.eq_ignore_ascii_case(BACKEND_TYPE) => {
            tr("wizard.value.proxy_mode.accel_redirect")
        }
        _ => tr("wizard.value.proxy_mode.redirect"),
    }
    .to_string();
    print_field_value_line(&proxy_mode);

    intro(
        tr("wizard.field.priority"),
        tr("wizard.prompt.node_priority_order"),
        Some("0"),
        None,
    );
    let priority: i32 = wiz_input_i32(0)?;
    intro(
        tr("wizard.field.client_speed_limit_kbs"),
        tr("wizard.prompt.client_speed_limit_kibs"),
        Some("0"),
        None,
    );
    let client_speed_limit_kbs: u64 = wiz_input_u64(0)?;
    intro(
        tr("wizard.field.client_burst_speed_kbs"),
        tr("wizard.prompt.client_burst_kibs"),
        Some("0"),
        None,
    );
    let client_burst_speed_kbs: u64 = wiz_input_u64(0)?;

    let path_rewrites = prompt_path_rewrites(
        tr("wizard.label.path_rewrite"),
        tr("wizard.prompt.path_rewrite_before_upstream"),
    )?;
    let anti_reverse_proxy =
        prompt_anti_reverse(tr("wizard.label.anti_reverse_proxy"))?;

    let (disk, open_list, direct_link, google_drive, webdav) =
        match backend_type.as_str() {
            "Disk" => {
                intro(
                    tr("wizard.field.description"),
                    tr("wizard.prompt.node_description_note"),
                    None,
                    Some(tr("wizard.example.disk.description").as_str()),
                );
                let description: String = wiz_input_string(None, true)?;
                (Some(Disk { description }), None, None, None, None)
            }
            "OpenList" => {
                intro(
                    tr("wizard.field.base_url"),
                    tr("wizard.prompt.alist_base_url"),
                    None,
                    Some(tr("wizard.example.url.local_emby").as_str()),
                );
                let b: String = wiz_input_string(None, false)?;
                intro(
                    tr("wizard.field.port"),
                    tr("wizard.prompt.alist_port_if_missing"),
                    None,
                    Some("5244"),
                );
                let p: String = wiz_input_string(None, true)?;
                intro(
                    tr("wizard.field.token"),
                    tr("wizard.prompt.alist_api_token"),
                    None,
                    None,
                );
                let tok: String = wiz_input_string(None, false)?;
                (
                    None,
                    Some(OpenList {
                        base_url: b,
                        port: p,
                        token: tok,
                    }),
                    None,
                    None,
                    None,
                )
            }
            "DirectLink" => {
                intro(
                    tr("wizard.field.user_agent"),
                    tr("wizard.prompt.direct_link_fetch_ua"),
                    None,
                    Some(tr("wizard.example.user_agent.mozilla").as_str()),
                );
                let ua: String = wiz_input_string(None, false)?;
                (None, None, Some(DirectLink { user_agent: ua }), None, None)
            }
            "googleDrive" => {
                print_subsection_title(tr(
                    "wizard.section.backend_node_google_drive",
                ));
                intro(
                    tr("wizard.field.node_uuid"),
                    tr("wizard.prompt.google_drive_node_uuid"),
                    None,
                    Some("google_drive_node_a"),
                );
                let node_uuid: String = input_text_w_echo(None, false)?;
                intro(
                    tr("wizard.field.drive_id"),
                    tr("wizard.prompt.google_drive_drive_id"),
                    None,
                    None,
                );
                let drive_id: String = input_text_w_echo(None, true)?;
                intro(
                    tr("wizard.field.drive_name"),
                    tr("wizard.prompt.google_drive_drive_name"),
                    None,
                    Some("SharedMedia"),
                );
                let drive_name: String = input_text_w_echo(None, true)?;
                intro(
                    tr("wizard.field.access_token"),
                    tr("wizard.prompt.google_drive_access_token"),
                    None,
                    None,
                );
                let access_token: String = input_text_w_echo(None, true)?;
                intro(
                    tr("wizard.field.client_id"),
                    tr("wizard.prompt.google_drive_client_id"),
                    None,
                    None,
                );
                let client_id: String = input_text_w_echo(None, true)?;
                intro(
                    tr("wizard.field.client_secret"),
                    tr("wizard.prompt.google_drive_client_secret"),
                    None,
                    None,
                );
                let client_secret: String = input_secret_w_echo(false)?;
                intro(
                    tr("wizard.field.refresh_token"),
                    tr("wizard.prompt.google_drive_refresh_token"),
                    None,
                    None,
                );
                let refresh_token: String = input_secret_w_echo(false)?;
                (
                    None,
                    None,
                    None,
                    Some(GoogleDriveConfig {
                        node_uuid,
                        client_id,
                        client_secret,
                        drive_id,
                        drive_name,
                        access_token,
                        refresh_token,
                        token: None,
                    }),
                    None,
                )
            }
            t if t.eq_ignore_ascii_case(BACKEND_TYPE) => {
                print_subsection_title(tr(
                    "wizard.section.backend_node_webdav",
                ));
                intro(
                    tr("wizard.field.url_mode"),
                    tr("wizard.hint.webdav_url_mode_values"),
                    Some("path_join"),
                    None,
                );
                let url_mode: String =
                    input_text_w_echo(Some("path_join".into()), false)?;
                intro(
                    tr("wizard.field.node_uuid"),
                    tr("wizard.prompt.webdav_node_uuid"),
                    None,
                    Some("webdav_node_a"),
                );
                let node_uuid: String = input_text_w_echo(None, true)?;
                intro(
                    tr("wizard.field.query_param"),
                    tr("wizard.prompt.webdav_query_param_key"),
                    Some("path"),
                    None,
                );
                let query_param: String =
                    input_text_w_echo(Some("path".into()), false)?;
                intro(
                    tr("wizard.field.url_template"),
                    tr("wizard.prompt.webdav_url_template"),
                    None,
                    None,
                );
                let url_template: String = input_text_w_echo(None, true)?;
                intro(
                    tr("wizard.field.username"),
                    tr("wizard.prompt.optional_http_basic_user"),
                    None,
                    None,
                );
                let username: String = input_text_w_echo(None, true)?;
                intro(
                    tr("wizard.field.password"),
                    tr("wizard.prompt.optional_http_basic_password"),
                    None,
                    None,
                );
                let password: String = input_secret_w_echo(true)?;
                intro(
                    tr("wizard.field.user_agent"),
                    tr("wizard.prompt.optional_custom_user_agent"),
                    None,
                    None,
                );
                let user_agent: String = input_text_w_echo(None, true)?;
                (
                    None,
                    None,
                    None,
                    None,
                    Some(WebDavConfig {
                        url_mode,
                        node_uuid,
                        query_param,
                        url_template,
                        username,
                        password,
                        user_agent,
                    }),
                )
            }
            _ => (None, None, None, None, None),
        };

    Ok(BackendNode {
        name,
        backend_type,
        pattern,
        pattern_regex: None,
        base_url,
        port,
        path,
        priority,
        proxy_mode,
        client_speed_limit_kbs,
        client_burst_speed_kbs,
        path_rewrites,
        anti_reverse_proxy,
        path_rewriter_cache: vec![],
        uuid: String::new(),
        disk,
        open_list,
        direct_link,
        google_drive,
        webdav,
    })
}

fn validate_and_preview(raw: &mut RawConfig, dest: &Path) -> Result<()> {
    resolve_dual_listen_ports(raw)?;
    finish_raw_config(dest.to_path_buf(), raw.clone())
        .map_err(|e| anyhow!("{e}"))?;
    let toml = emit_wizard_config_toml(raw)?;
    print_title(tr("wizard.section.preview_toml"));
    print!("{toml}");
    std::io::stdout().flush()?;
    Ok(())
}

fn save_config_file(dest: &Path, raw: &mut RawConfig) -> Result<()> {
    resolve_dual_listen_ports(raw)?;
    finish_raw_config(dest.to_path_buf(), raw.clone())
        .map_err(|e| anyhow!("{e}"))?;
    let toml = emit_wizard_config_toml(raw)?;
    write_atomic(dest, &toml).map_err(|e| anyhow!("{e}"))
}

#[derive(Clone, Copy)]
enum EditMenuChoice {
    Shared,
    Frontend,
    Backend,
    BackendNodes,
    Done,
}

fn run_edit_loop(mut raw: RawConfig) -> Result<RawConfig> {
    loop {
        let mut choices: Vec<EditMenuChoice> = vec![EditMenuChoice::Shared];
        let mut labels: Vec<String> = vec![tr("wizard.menu.edit_shared_group")];
        if raw.frontend.is_some() {
            choices.push(EditMenuChoice::Frontend);
            labels.push(tr("wizard.menu.frontend_section"));
        }
        if raw.backend.is_some() {
            choices.push(EditMenuChoice::Backend);
            labels.push(tr("wizard.menu.backend_section"));
            choices.push(EditMenuChoice::BackendNodes);
            labels.push(tr("wizard.menu.backend_nodes"));
        }
        choices.push(EditMenuChoice::Done);
        labels.push(tr("wizard.menu.done_save"));
        print_field_intro_line(
            tr("wizard.prompt.edit_which_part"),
            tr("wizard.prompt.choose_config_section"),
            None,
            None,
        );
        let sel = Select::with_theme(&theme())
            .with_prompt("")
            .items(&labels)
            .report(false)
            .interact()
            .map_err(|e| anyhow!(e.to_string()))?;

        match choices[sel] {
            EditMenuChoice::Shared => {
                prompt_shared_sections(&mut raw)?;
            }
            EditMenuChoice::Frontend => {
                raw.frontend = Some(prompt_frontend_section()?);
            }
            EditMenuChoice::Backend => {
                raw.backend = Some(prompt_backend_section()?);
            }
            EditMenuChoice::BackendNodes => {
                raw.backend_nodes = Some(prompt_backend_nodes_loop()?);
            }
            EditMenuChoice::Done => break,
        }
    }
    Ok(raw)
}

#[cfg(test)]
mod normalize_emby_url_tests {
    use super::normalize_emby_url;

    #[test]
    fn empty_and_whitespace_default() {
        assert_eq!(normalize_emby_url(""), "http://127.0.0.1");
        assert_eq!(normalize_emby_url("   "), "http://127.0.0.1");
    }

    #[test]
    fn prepends_http_without_scheme() {
        assert_eq!(normalize_emby_url("127.0.0.1"), "http://127.0.0.1");
        assert_eq!(
            normalize_emby_url("192.168.1.1:8096"),
            "http://192.168.1.1:8096"
        );
    }

    #[test]
    fn keeps_explicit_scheme() {
        assert_eq!(
            normalize_emby_url("https://emby.example.com"),
            "https://emby.example.com"
        );
    }
}
