use anyhow::Result;
use clap::Parser;
use prism_mcp::{
    init_logging, log_process_start, log_top_level_error, serve_with_mode, PrismMcpCli,
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = PrismMcpCli::parse();
    init_logging(&cli)?;
    let root = cli.root.canonicalize().unwrap_or_else(|_| cli.root.clone());
    log_process_start(&cli, &root);
    match serve_with_mode(cli.clone()).await {
        Ok(()) => Ok(()),
        Err(error) => {
            log_top_level_error(&cli, &root, &error);
            Err(error)
        }
    }
}
