use anyhow::Result;
use clap::Parser;
use prism_mcp::{PrismMcpCli, PrismMcpServer};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = PrismMcpCli::parse();
    let server = PrismMcpServer::from_workspace(cli.root)?;
    server.serve_stdio().await
}
