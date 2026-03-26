use anyhow::Result;
use clap::Parser;
use prism_mcp::{PrismMcpCli, PrismMcpServer};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = PrismMcpCli::parse();
    let features = cli.features();
    let server = PrismMcpServer::from_workspace_with_features(cli.root, features)?;
    server.serve_stdio().await
}
