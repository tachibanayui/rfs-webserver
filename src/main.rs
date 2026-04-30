mod cli;
mod dictionary;
mod routes;
mod vfs;

use std::net::SocketAddr;

use axum::Router;
use clap::Parser;

use crate::cli::Args;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();
    let config = args.into_config()?;
    let filesystem = vfs::generator::generate(&config);
    let app: Router = routes::router(filesystem, config.footer_signature.clone());

    let address: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;

    println!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
