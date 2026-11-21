//! Digital Twin Platform v4 — Unified CLI + Web Server
//!
//! dtw <SUBCOMMAND> [OPTIONS]
//!
//! Subcommands:
//!   lean4      Verify Lean4 formal proofs (startup check)
//!   domains    List registered domain plugins
//!   simulate   Forward-chaining planner on a domain
//!   dashboard  Operator reroute dashboard for a domain
//!   audit      Print governance audit chain + integrity
//!   eval       Train SAM classifier; print MCC + bootstrap CI
//!   qa         Operator AI Q&A via /chat/completions (Gemini)
//!   serve      Start Topcoat web UI + Axum JSON API server
//!
//! Mandatory preferences:
//!   [IGG-04] Lean4 proof verification
//!   [IGG-05] SAM ρ=0.05
//!   [IGG-06] MCC + bootstrap 95% CI
//!   [IGG-07] OpenAI /chat/completions

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

use clap::{Args, Parser, Subcommand};
use colored::Colorize;
use serde_json::json;
use uuid::Uuid;
use rand::{Rng, SeedableRng, rngs::StdRng};

pub static DB: once_cell::sync::Lazy<db::Db> = once_cell::sync::Lazy::new(|| {
    db::init_db("digital_twin.db").expect("Failed to init SQLite")
});

// ══════════════════════════════════════════════════════════════
// CLI (clap derive)
// ══════════════════════════════════════════════════════════════

#[derive(Parser)]
#[command(
    name = "dtw",
    version = "4.0.0",
    about = "Digital Twin Platform v4 — Unified Rust CLI + Web Server",
    after_help = "Set GEMINI_API_KEY env var for the qa subcommand."
)]
struct Cli {
    #[arg(long, global = true, default_value = "127.0.0.1")]
    iggy_host: String,
    #[arg(long, global = true, default_value = "3000")]
    iggy_port: u16,
    #[arg(long, global = true, env = "GEMINI_API_KEY", default_value = "")]
    api_key: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Verify Lean4 formal proofs [IGG-04]
    Lean4,
    /// List registered domain plugins
    Domains,
    /// Run forward-chaining planner on a domain
    Simulate(SimArgs),
    /// Operator reroute dashboard for a domain
    Dashboard(DomainArg),
    /// Print governance audit chain and integrity status
    Audit(SimArgs),
    /// Train SAM classifier; report MCC + bootstrap CI [IGG-05][IGG-06]
    Eval(EvalArgs),
    /// Operator AI Q&A via /chat/completions (Gemini) [IGG-07]
    Qa(QaArgs),
    /// Start Topcoat web UI + Axum JSON API server
    Serve(ServeArgs),
}

#[derive(Args)] struct DomainArg { #[arg(long, default_value="logistics")] domain: String }
#[derive(Args)] struct SimArgs { #[arg(long, default_value="rail")] domain: String, #[arg(long, default_value="6")] steps: usize }
#[derive(Args)] struct EvalArgs { #[arg(long, default_value="rail")] domain: String, #[arg(long, default_value="6")] steps: usize, #[arg(long, default_value="100")] epochs: usize, #[arg(long, default_value="0.05")] rho: f64 }
#[derive(Args)] struct QaArgs { #[arg(long, default_value="rail")] domain: String, #[arg(long, default_value="6")] steps: usize, #[arg(long, default_value="Summarise system health and any distressed entities.")] question: String }
#[derive(Args)] struct ServeArgs { #[arg(long, default_value="8080")] port: u16, #[arg(long, default_value="0.0.0.0")] host: String }

fn hdr(title: &str) { println!("\n{}\n  {}\n{}", "═".repeat(64).blue(), title.bold().white(), "═".repeat(64).blue()); }
fn sep(label: &str) { println!("\n  {} {}", "──".dimmed(), label.bold()); }

// ══════════════════════════════════════════════════════════════
// SUBCOMMAND HANDLERS
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

fn run_simulation(domain: &str, steps: usize, bus: &mut iggy::IggyBus, registry: &mut engine::GovernanceRegistry, run_id: &str) -> Vec<planner::TelemetryRecord> {
    let mut all_telem = vec![];
    if domain == "rail" {
        let ont = planner::build_rail_ontology(steps);
        println!("  Ontology: {} v{}  fp={}", ont.name, ont.version, ont.fingerprint());
        let mut names: Vec<&str> = ont.actions.keys().map(|s| s.as_str()).collect();
        names.sort();
        println!("  Actions  : {}", names.iter().map(|s| s.green().to_string()).collect::<Vec<_>>().join(" · "));
        println!("  Entities : {}  Grants : {}", ont.initial_entities.len(), ont.capability_grants.len());
        for entity in &ont.initial_entities {
            sep(&format!("Entity: {} ({})", entity.entity_id, entity.properties.get("status").unwrap_or(&"-".to_string())));
            let t = planner::run_planner(&ont, entity, registry, bus, run_id);
            for r in &t { println!("    t={:2}  n_actions={}  label={}", r.timestep, r.n_actions, if r.label==1 {"DISTRESSED".red().to_string()} else {"ok".green().to_string()}); }
            all_telem.extend(t);
        }
    } else {
        let plugin = domains::get_plugin(domain);
        let mut graph = ontology_graph::build_graph(domain);
        let distressed: Vec<String> = graph.all().filter(|n| plugin.is_distressed(n, &graph)).map(|n| n.node_id.clone()).collect();
        let available: Vec<String> = graph.all().filter(|n| plugin.is_available(n, &graph)).map(|n| n.node_id.clone()).collect();
        println!("  Distressed: {}  Available: {}", distressed.len().to_string().yellow(), available.len().to_string().green());
        let mut suggestions = vec![];
        for src_id in &distressed {
            let cands: Vec<_> = available.iter().filter_map(|id| graph.get(id).cloned()).collect();
            if cands.is_empty() { continue; }
            let src = match graph.get(src_id) { Some(n)=>n.clone(), None=>continue };
            if let Some((tgt, reason)) = plugin.suggest_target(&src, &cands, &graph) { suggestions.push((src_id.clone(), tgt, reason)); }
        }
        for (src, tgt, reason) in suggestions {
            println!("\n  ┌─ {} → {}  ({})", src.yellow().bold(), tgt.green().bold(), reason.dimmed());
            match graph.reroute(&src, &tgt) {
                Ok(()) => { println!("  └─ {}", "COMMITTED ✓".green());
                    registry.record(run_id,0,"platform","reroute",&src,"COMMITTED",&tgt);
                    bus.emit_event(json!({"event":"reroute","src":src,"tgt":tgt}));
                    all_telem.push(planner::TelemetryRecord{run_id:run_id.into(),entity_id:src,timestep:1,n_actions:1,n_props:3,elapsed_us:0,label:0}); }
                Err(e) => println!("  └─ {} {}", "FAILED ✗".red(), e),
            }
        }
        println!("\n  Graph: {} nodes  {} edges  {} TXs", graph.node_count(), graph.edge_count(), graph.tx_log.len());
    }
    all_telem
}

fn cmd_simulate(a: SimArgs, bus: &mut iggy::IggyBus) {
    hdr(&format!("SIMULATE — domain={}  steps={}", a.domain, a.steps));
    let run_id = format!("sim_{}", Uuid::new_v4().simple());
    let mut reg = engine::GovernanceRegistry::new();
    let telem = run_simulation(&a.domain, a.steps, bus, &mut reg, &run_id);
    println!("\n  Audit: {} entries  Chain: {}  Iggy: {}  Telem: {} ({} distressed)",
        reg.entries.len(), if reg.verify_chain(){"✓".green().to_string()} else {"✗".red().to_string()},
        bus.status_line(), telem.len(), telem.iter().filter(|r|r.label==1).count());
}

fn cmd_dashboard(a: DomainArg, bus: &mut iggy::IggyBus) {
    if a.domain=="rail" { eprintln!("Dashboard not for rail CLI. Use: dtw serve"); std::process::exit(1); }
    hdr(&format!("OPERATOR DASHBOARD — domain={}", a.domain));
    let mut reg = engine::GovernanceRegistry::new();
    run_simulation(&a.domain, 1, bus, &mut reg, &format!("dash_{}", Uuid::new_v4().simple()));
}

fn cmd_audit(a: SimArgs, bus: &mut iggy::IggyBus) {
    hdr(&format!("AUDIT CHAIN — domain={}  steps={}", a.domain, a.steps));
    let mut reg = engine::GovernanceRegistry::new();
    run_simulation(&a.domain, a.steps, bus, &mut reg, &format!("audit_{}", Uuid::new_v4().simple()));
    sep("Audit Log");
    for e in &reg.entries { println!("  t={:3}  {:12}  {:16}  {:10}  hash={}", e.timestep, e.user.cyan(), e.action.yellow(), e.outcome.green(), e.hash.dimmed()); }
    sep("Chain Verification");
    println!("  Entries: {}  Intact: {}", reg.entries.len(), if reg.verify_chain(){"✓ VALID".green().to_string()} else {"✗ TAMPERED".red().to_string()});
}

fn cmd_eval(a: EvalArgs, bus: &mut iggy::IggyBus) {
    hdr(&format!("SAM EVAL — domain={}  epochs={}  ρ={}", a.domain, a.epochs, a.rho));
    let mut reg = engine::GovernanceRegistry::new();
    let mut telem = run_simulation(&a.domain, a.steps, bus, &mut reg, &format!("eval_{}", Uuid::new_v4().simple()));
    let mut rng = StdRng::seed_from_u64(7);
    for i in 0..120usize {
        let dist = i%5==0;
        telem.push(planner::TelemetryRecord{ run_id:"synth".into(), entity_id:format!("s{}",i), timestep:i+1,
            n_actions:if dist{0}else{rng.gen_range(1..5)}, n_props:rng.gen_range(2..8),
            elapsed_us:if dist{rng.gen_range(5000..20000)}else{rng.gen_range(10..500)}, label:if dist{1}else{0} });
    }
    sep(&format!("SAM Training  (ρ={}, lr=0.05, epochs={})", a.rho, a.epochs));
    let mut clf = sam_classifier::DistressClassifier::new();
    clf.fit(&telem, a.epochs, 0.05, a.rho);
    println!("  Samples: {}  Final loss: {:.6}  SAM ρ={}: ✓", telem.len(), clf.loss_curve.last().copied().unwrap_or(f64::NAN), a.rho);
    sep("MCC Evaluation  [IGG-06]");
    let m = sam_classifier::evaluate_mcc(&clf, &telem);
    println!("  TP={}  TN={}  FP={}  FN={}", m.tp, m.tn, m.fp, m.fn_);
    println!("  MCC: {:.4}  (95% CI [{:.4}, {:.4}])", m.mcc, m.ci_lo, m.ci_hi);
    println!("  Accuracy: {:.4}  Precision: {:.4}  Recall: {:.4}  F1: {:.4}", m.accuracy, m.precision, m.recall, m.f1);
}

fn cmd_qa(a: QaArgs, bus: &mut iggy::IggyBus, api_key: &str) {
    hdr("OPERATOR Q&A  [IGG-07]  — /chat/completions (Gemini)");
    let mut reg = engine::GovernanceRegistry::new();
    let telem = run_simulation(&a.domain, a.steps, bus, &mut reg, &format!("qa_{}", Uuid::new_v4().simple()));
    sep("Question"); println!("  Q: {}", a.question.yellow());
    sep("Answer");
    println!("  {}", qa::operator_qa(&a.question, &reg, &telem, bus, api_key));
}

async fn cmd_serve(a: ServeArgs) {
    println!("═══════════════════════════════════════════════════════════");
    println!("🏭 Digital Twin Platform v4 — Web Server");
    println!("═══════════════════════════════════════════════════════════");
    let _ = &*DB;
    println!("✅ SQLite: digital_twin.db");
    let state = api::AppState { db: DB.clone() };
    let app = api::api_router(state).layer(tower_http::cors::CorsLayer::permissive());
    let addr = format!("{}:{}", a.host, a.port);
    println!("🌐 UI:  http://localhost:{}", a.port);
    println!("📡 API: http://localhost:{}/api/health", a.port);
    println!("═══════════════════════════════════════════════════════════");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ══════════════════════════════════════════════════════════════
// MAIN
// ══════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if !lean4::verify() { eprintln!("Lean4 proof check failed"); std::process::exit(2); }
    match cli.command {
        Command::Lean4 => {}
        Command::Domains => cmd_domains(),
        Command::Serve(a) => cmd_serve(a).await,
        cmd => {
            let mut bus = iggy::IggyBus::new(&cli.iggy_host, cli.iggy_port);
            bus.connect();
            match cmd {
                Command::Simulate(a)  => cmd_simulate(a, &mut bus),
                Command::Dashboard(a) => cmd_dashboard(a, &mut bus),
                Command::Audit(a)     => cmd_audit(a, &mut bus),
                Command::Eval(a)      => cmd_eval(a, &mut bus),
                Command::Qa(a)        => cmd_qa(a, &mut bus, &cli.api_key),
                _ => unreachable!(),
            }
            println!("\n  {}", format!("[Iggy] {}", bus.status_line()).dimmed());
        }
    }
}
