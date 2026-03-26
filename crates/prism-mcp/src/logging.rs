use std::env;
use std::io::{self, IsTerminal};
use std::path::Path;

use anyhow::{Context, Error, Result};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::{PrismMcpCli, PrismMcpMode};

pub fn init_logging(cli: &PrismMcpCli) -> Result<()> {
    let env_filter = env_filter(default_filter(cli.mode))?;
    let use_json = match env::var("PRISM_LOG_FORMAT") {
        Ok(value) if value.eq_ignore_ascii_case("json") => true,
        Ok(value) if value.eq_ignore_ascii_case("text") => false,
        Ok(value) => {
            return Err(anyhow::anyhow!(
                "unsupported PRISM_LOG_FORMAT `{value}`; expected `json` or `text`"
            ));
        }
        Err(_) => !io::stderr().is_terminal(),
    };

    if use_json {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_file(true)
            .with_line_number(true)
            .json()
            .flatten_event(true)
            .with_current_span(false)
            .with_span_list(true)
            .with_writer(io::stderr)
            .try_init()
            .map_err(|error| anyhow::anyhow!("failed to initialize JSON logger: {error}"))?;
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_file(true)
            .with_line_number(true)
            .with_ansi(io::stderr().is_terminal())
            .compact()
            .with_writer(io::stderr)
            .try_init()
            .map_err(|error| anyhow::anyhow!("failed to initialize text logger: {error}"))?;
    }

    Ok(())
}

pub fn log_process_start(cli: &PrismMcpCli, root: &Path) {
    info!(
        mode = %mode_name(cli.mode),
        root = %root.display(),
        coordination = %cli.features().mode_label(),
        http_bind = %cli.http_bind,
        http_path = %cli.http_path,
        health_path = %cli.health_path,
        uri_file = %cli.http_uri_file_path(root).display(),
        log_path = %cli.log_path(root).display(),
        "starting prism-mcp"
    );
}

pub fn log_top_level_error(cli: &PrismMcpCli, root: &Path, error_value: &Error) {
    error!(
        mode = %mode_name(cli.mode),
        root = %root.display(),
        error = %error_value,
        error_chain = %format_error_chain(error_value),
        "prism-mcp exited with error"
    );
}

pub(crate) fn format_error_chain(error: &Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn env_filter(default_directive: &str) -> Result<EnvFilter> {
    if let Ok(value) = env::var("PRISM_LOG") {
        return EnvFilter::try_new(value)
            .context("failed to parse PRISM_LOG as a tracing filter directive");
    }
    if let Ok(value) = env::var("RUST_LOG") {
        return EnvFilter::try_new(value)
            .context("failed to parse RUST_LOG as a tracing filter directive");
    }
    Ok(EnvFilter::new(default_directive))
}

fn default_filter(mode: PrismMcpMode) -> &'static str {
    match mode {
        PrismMcpMode::Stdio => "warn",
        PrismMcpMode::Daemon | PrismMcpMode::Bridge => "info",
    }
}

fn mode_name(mode: PrismMcpMode) -> &'static str {
    match mode {
        PrismMcpMode::Stdio => "stdio",
        PrismMcpMode::Daemon => "daemon",
        PrismMcpMode::Bridge => "bridge",
    }
}
