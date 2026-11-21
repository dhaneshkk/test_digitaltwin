//! Digital Twin Platform v4 — Unified CLI + Web Server
//!
//! All operations route through service.rs → SQLite + optional Iggy.
//! No split data paths. CLI and web API share the same DB.
//!
//! dtw lean4                            [IGG-04]
//! dtw domains
//! dtw simulate --domain rail --steps 6
//! dtw audit    --domain incident
//! dtw eval     --domain rail --epochs 120 --rho 0.05  [IGG-05][IGG-06]
//! dtw qa       --question "..."        [IGG-07]
//! dtw serve    --port 8080

mod api;
mod db;
mod domains;
mod engine;
mod iggy;
mod lean4;
mod models;
mod ontology_graph;
mod pages;
mod planner;
mod qa;
mod sam_classifier;
mod service;

use clap::{Args, Parser, Subcommand};
use colored::Colorize;
use service::ServiceCtx;

pub static DB: once_cell::sync::Lazy<db::Db> = once_cell::sync::Lazy::new(|| {
    db::init_db("digital_twin.db").expect("Failed to init SQLite")
});

#[derive(Parser)]
#[command(name="dtw", version="4.0.0", about="Digital Twin Platform v4 — Unified CLI + Web Server")]
struct Cli {
    #[arg(long, global=true, default_value="127.0.0.1")] iggy_host: String,
    #[arg(long, global=true, default_value="3000")]       iggy_port: u16,
    #[arg(long, global=true, env="GEMINI_API_KEY", default_value="")] api_key: String,
    #[command(subcommand)] command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Verify Lean4 formal proofs [IGG-04]
    Lean4,
    /// List registered domain plugins
    Domains,
    /// Run forward-chaining planner on a domain (writes to SQLite + Iggy)
    Simulate(SimArgs),
    /// Operator reroute dashboard for a domain
    Dashboard(DomainArg),
    /// Print governance audit chain from SQLite
    Audit(SimArgs),
    /// Train SAM classifier; report MCC + bootstrap CI [IGG-05][IGG-06]
    Eval(EvalArgs),
    /// Operator AI Q&A via /chat/completions (Gemini) [IGG-07]
    Qa(QaArgs),
    /// Start Topcoat web UI + Axum JSON API server
    Serve(ServeArgs),
}

#[derive(Args)] struct DomainArg { #[arg(long, default_value="logistics")] domain: String }
#[derive(Args)] struct SimArgs   { #[arg(long, default_value="rail")] domain: String, #[arg(long, default_value="6")] steps: usize }
#[derive(Args)] struct EvalArgs  { #[arg(long, default_value="rail")] domain: String, #[arg(long, default_value="6")] steps: usize, #[arg(long, default_value="100")] epochs: usize, #[arg(long, default_value="0.05")] rho: f64 }
#[derive(Args)] struct QaArgs    { #[arg(long, default_value="rail")] domain: String, #[arg(long, default_value="6")] steps: usize, #[arg(long, default_value="Summarise system health and any distressed entities.")] question: String }
#[derive(Args)] struct ServeArgs { #[arg(long, default_value="8080")] port: u16, #[arg(long, default_value="0.0.0.0")] host: String }

fn hdr(title: &str) { println!("\n{}\n  {}\n{}", "═".repeat(64).blue(), title.bold().white(), "═".repeat(64).blue()); }
fn sep(label: &str) { println!("\n  {} {}", "──".dimmed(), label.bold()); }

// ══════════════════════════════════════════════════════════════
// CLI handlers — all go through service layer now
// ══════════════════════════════════════════════════════════════

fn cmd_domains() {
    hdr("REGISTERED DOMAIN PLUGINS");
    for p in domains::list_plugins() {
        println!("  {}  {}  ({} → {})", "✓".green(), p.name().yellow().bold(), p.source_type().cyan(), p.target_type().cyan());
        println!("     {}", p.description().dimmed());
    }
    println!("\n  {}  {}  (EntityState → BusinessOntology actions)", "✓".green(), "rail".yellow().bold());
    println!("     {}", "Rail fleet — depart / arrive / clear_delay / emergency_stop".dimmed());
}

fn cmd_simulate(a: SimArgs, ctx: &mut ServiceCtx) {
    hdr(&format!("SIMULATE — domain={}  steps={}", a.domain, a.steps));
    match service::plan_domain(ctx, &a.domain, a.steps) {
        Ok(result) => {
            sep("Results");
            for r in &result.telemetry {
                println!("    t={:2}  entity={}  n_actions={}  label={}",
                    r.timestep, r.entity_id,  r.n_actions,
                    if r.label==1 {"DISTRESSED".red().to_string()} else {"ok".green().to_string()});
            }
            println!("\n  Run ID      : {}", result.run_id);
            println!("  Entities    : {}", result.n_entities);
            println!("  Telemetry   : {} records ({} distressed)", result.telemetry.len(), result.n_distressed);
            println!("  Audit       : {} entries  Chain: {}",
                result.audit_entries,
                if result.chain_valid {"✓".green().to_string()} else {"✗".red().to_string()});
            println!("  Persisted   : ✓ SQLite (digital_twin.db)");
        }
        Err(e) => println!("  {} {}", "ERROR".red(), e),
    }
}

fn cmd_dashboard(a: DomainArg, ctx: &mut ServiceCtx) {
    if a.domain == "rail" { eprintln!("Dashboard not for rail CLI. Use: dtw serve"); std::process::exit(1); }
    hdr(&format!("OPERATOR DASHBOARD — domain={}", a.domain));
    match service::plan_domain(ctx, &a.domain, 1) {
        Ok(r) => println!("  {} committed  {} audit entries  persisted to SQLite",
            r.telemetry.len(), r.audit_entries),
        Err(e) => println!("  {} {}", "ERROR".red(), e),
    }
}

fn cmd_audit(a: SimArgs, ctx: &mut ServiceCtx) {
    hdr(&format!("AUDIT CHAIN — domain={}", a.domain));
    // First ensure the domain ontology exists in SQLite
    let _ = service::plan_domain(ctx, &a.domain, a.steps);

    // Now read audit from SQLite (the shared store)
    let ont_id = {
        let list = db::list_ontologies(&ctx.db).unwrap_or_default();
        list.first().and_then(|o| o["id"].as_str().map(|s| s.to_string())).unwrap_or_default()
    };

    sep("Audit Log (from SQLite)");
    let entries = db::list_audit(&ctx.db, &ont_id, 50).unwrap_or_default();
    for e in &entries {
        println!("  t={:3}  {:12}  {:16}  {:10}  hash={}",
            e["timestep"].as_i64().unwrap_or(0),
            e["user_id"].as_str().unwrap_or("").cyan(),
            e["action"].as_str().unwrap_or("").yellow(),
            e["outcome"].as_str().unwrap_or("").green(),
            e["chain_hash"].as_str().unwrap_or("").dimmed());
    }

    sep("Chain Verification");
    match service::verify_audit(ctx, &ont_id) {
        Ok((valid, count)) => println!("  Entries: {}  Intact: {}",
            count, if valid {"✓ VALID".green().to_string()} else {"✗ TAMPERED".red().to_string()}),
        Err(e) => println!("  {} {}", "ERROR".red(), e),
    }

    sep("User Statistics");
    if let Ok((stats, patterns)) = service::audit_stats(ctx, &ont_id) {
        for s in &stats {
            println!("  {} : {}/{} success ({:.0}%)", s.user_id, s.successes, s.total, s.success_rate);
        }
        if !patterns.is_empty() {
            sep("Failure Patterns");
            for p in &patterns {
                println!("  [{}x] {} → {} : {}", p.count, p.action, p.outcome, p.detail);
            }
        }
    }
}

fn cmd_eval(a: EvalArgs, ctx: &mut ServiceCtx) {
    hdr(&format!("SAM EVAL — domain={}  epochs={}  ρ={}", a.domain, a.epochs, a.rho));
    match service::run_eval(ctx, &a.domain, a.steps, a.epochs, a.rho) {
        Ok(result) => {
            sep(&format!("SAM Training  (ρ={}, lr=0.05, epochs={})", a.rho, a.epochs));
            println!("  Samples: {}  Final loss: {:.6}  SAM ρ={}: ✓",
                result.n_samples,
                result.loss_curve.last().copied().unwrap_or(f64::NAN),
                a.rho);
            sep("MCC Evaluation  [IGG-06]");
            let m = &result.mcc;
            println!("  TP={}  TN={}  FP={}  FN={}", m.tp, m.tn, m.fp, m.fn_);
            println!("  MCC: {:.4}  (95% CI [{:.4}, {:.4}])", m.mcc, m.ci_lo, m.ci_hi);
            println!("  Accuracy: {:.4}  Precision: {:.4}  Recall: {:.4}  F1: {:.4}",
                m.accuracy, m.precision, m.recall, m.f1);
            println!("  Persisted: ✓ SQLite (digital_twin.db)");
        }
        Err(e) => println!("  {} {}", "ERROR".red(), e),
    }
}

fn cmd_qa(a: QaArgs, ctx: &mut ServiceCtx, api_key: &str) {
    hdr("OPERATOR Q&A  [IGG-07]  — /chat/completions (Gemini)");
    match service::operator_qa(ctx, &a.domain, a.steps, &a.question, api_key) {
        Ok(answer) => {
            sep("Question"); println!("  Q: {}", a.question.yellow());
            sep("Answer");   println!("  {}", answer);
        }
        Err(e) => println!("  {} {}", "ERROR".red(), e),
    }
}

async fn cmd_serve(a: ServeArgs) {
    println!("═══════════════════════════════════════════════════════════");
    println!("🏭 Digital Twin Platform v4 — Web Server");
    println!("═══════════════════════════════════════════════════════════");
    let _ = &*DB;
    println!("✅ SQLite: digital_twin.db");
    let state = api::AppState { db: DB.clone() };
    let app = api::api_router(state).layer(tower_http::cors::CorsLayer::permissive());
    println!("🌐 UI:  http://localhost:{}", a.port);
    println!("📡 API: http://localhost:{}/api/health", a.port);
    println!("═══════════════════════════════════════════════════════════");
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", a.host, a.port)).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ══════════════════════════════════════════════════════════════
// MAIN — all CLI paths construct ServiceCtx first
// ══════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if !lean4::verify() { eprintln!("Lean4 proof check failed"); std::process::exit(2); }

    match cli.command {
        Command::Lean4   => {}
        Command::Domains => cmd_domains(),
        Command::Serve(a) => cmd_serve(a).await,
        cmd => {
            // All CLI subcommands share one ServiceCtx → one DB + one Iggy
            let _ = &*DB;
            let mut ctx = ServiceCtx::new(DB.clone())
                .with_iggy(&cli.iggy_host, cli.iggy_port);

            match cmd {
                Command::Simulate(a)  => cmd_simulate(a, &mut ctx),
                Command::Dashboard(a) => cmd_dashboard(a, &mut ctx),
                Command::Audit(a)     => cmd_audit(a, &mut ctx),
                Command::Eval(a)      => cmd_eval(a, &mut ctx),
                Command::Qa(a)        => cmd_qa(a, &mut ctx, &cli.api_key),
                _ => unreachable!(),
            }
            if let Some(bus) = &ctx.bus {
                println!("\n  {}", format!("[Iggy] {}", bus.status_line()).dimmed());
            }
        }
    }
}
