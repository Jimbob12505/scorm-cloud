use axum::{routing::{get}, Router};
use std::env;
use tokio::net::TcpListener;
use tower_http::{trace::TraceLayer, cors::{Any, CorsLayer}};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use axum::extract::DefaultBodyLimit;

mod db;
mod models;
mod routes;
mod manifest;
mod runtime;
mod util;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            env::var("RUST_LOG").unwrap_or_else(|_| "rustiscorm_runtime=info,axum=info".into())
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let pool = db::connect().await?;
    // crate-relative path for sqlx migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(routes::router(pool.clone()))
        .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any));

    let port: u16 = env::var("PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8081);
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("listening on http://0.0.0.0:{}", port);

    axum::serve(listener, app).await?;
    Ok(())
}

