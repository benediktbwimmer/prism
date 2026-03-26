use anyhow::Result;
use clap::Parser;
use prism_mcp::{serve_with_mode, PrismMcpCli};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = PrismMcpCli::parse();
    serve_with_mode(cli).await
}
