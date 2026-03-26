use anyhow::Result;
use clap::Parser;
use prism_mcp::{
    init_logging, log_process_start, log_top_level_error, maybe_daemonize_process, serve_with_mode,
    PrismMcpCli,
};

fn main() -> Result<()> {
    let cli = PrismMcpCli::parse();
    maybe_daemonize_process(&cli)?;
    init_logging(&cli)?;
    let root = cli.root.canonicalize().unwrap_or_else(|_| cli.root.clone());
    log_process_start(&cli, &root);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match runtime.block_on(serve_with_mode(cli.clone())) {
        Ok(()) => Ok(()),
        Err(error) => {
            log_top_level_error(&cli, &root, &error);
            Err(error)
        }
    }
}
