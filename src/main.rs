//! Digital Twin Engine v3 — Rust + Topcoat + SQLite
//!
//! Architecture:
//!   - Pure Rust state machine (no Clingo/ASP C dependency)
//!   - SQLite persistence via rusqlite (bundled, zero config)
//!   - Topcoat full-stack UI (server-rendered + JS reactivity)
//!   - Axum JSON API layer alongside Topcoat pages
//!   - SHA-256 chained audit trail
//!   - SAM robustness analysis with bootstrap MCC CI
//!
//! Run:
//!   cargo run
//!   → http://localhost:3000       (Topcoat UI)
//!   → http://localhost:3000/api/  (JSON API)

mod api;
mod db;
mod engine;
mod models;
mod pages;

use std::sync::{Arc, Mutex};

/// Global DB handle — in production, use Topcoat's context injection.
pub static DB: once_cell::sync::Lazy<db::Db> = once_cell::sync::Lazy::new(|| {
    db::init_db("digital_twin.db").expect("Failed to init SQLite")
});

#[tokio::main]
async fn main() {
    println!("═══════════════════════════════════════════════════════════");
    println!("🏭 Digital Twin Engine v3 — Rust + Topcoat + SQLite");
    println!("═══════════════════════════════════════════════════════════");

    // Force DB init
    let _ = &*DB;
    println!("✅ SQLite database initialised: digital_twin.db");

    // Build combined router: Topcoat pages + Axum JSON API
    let state = api::AppState { db: DB.clone() };

    // Topcoat pages
    let topcoat_router = topcoat::Router::builder()
        .discover()
        .build();

    // Axum API routes
    let api_routes = api::api_router(state);

    // Merge: API routes take precedence for /api/* paths
    let app = api_routes.merge(topcoat_router);

    // Add CORS for development
    let app = app.layer(
        tower_http::cors::CorsLayer::permissive()
    );

    let addr = "0.0.0.0:3000";
    println!("🌐 Server: http://localhost:3000");
    println!("📡 API:    http://localhost:3000/api/health");
    println!("═══════════════════════════════════════════════════════════");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
