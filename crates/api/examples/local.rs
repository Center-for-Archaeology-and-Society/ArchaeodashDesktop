//! Local-only API harness for development and end-to-end verification.
//! Usage: cargo run -p archaeodash-api --example local -- /path/to/project
use archaeodash_api::{root_router, AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("provide a project directory")?;
    let state = AppState::open(std::path::Path::new(&root))?;
    let addr = std::env::var("LOCAL_API_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    eprintln!("Local API listening on http://{addr}");
    axum::serve(listener, root_router(state)).await?;
    Ok(())
}
