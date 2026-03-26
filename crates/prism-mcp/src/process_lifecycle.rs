use anyhow::{anyhow, Context, Result};

use crate::{PrismMcpCli, PrismMcpMode};

pub fn maybe_daemonize_process(cli: &PrismMcpCli) -> Result<()> {
    if !cli.daemonize {
        return Ok(());
    }
    if cli.mode != PrismMcpMode::Daemon {
        return Err(anyhow!("--daemonize is only supported with --mode daemon"));
    }
    daemonize_process()
}

#[cfg(unix)]
fn daemonize_process() -> Result<()> {
    unsafe {
        let fork_result = libc::fork();
        if fork_result < 0 {
            return Err(std::io::Error::last_os_error()).context("failed to fork prism-mcp");
        }
        if fork_result > 0 {
            libc::_exit(0);
        }
        if libc::setsid() < 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to create prism-mcp session");
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn daemonize_process() -> Result<()> {
    Err(anyhow!("daemonization is only supported on unix platforms"))
}
