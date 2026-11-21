//! JSON API layer (Axum) — /api/* routes alongside Topcoat pages.
//!
//! POST   /api/ontologies              Create
//! GET    /api/ontologies              List
//! GET    /api/ontologies/:id          Get full
//! DELETE /api/ontologies/:id          Delete
//! POST   /api/ontologies/:id/validate Validate
//! GET    /api/ontologies/:id/ifd      IFD priority matrix
//! GET    /api/ontologies/:id/caps     Capability matrix
//! POST   /api/ontologies/:id/simulate Run simulation
//! GET    /api/ontologies/:id/runs     List runs
//! GET    /api/runs/:id                Get run detail
//! GET    /api/ontologies/:id/audit    Audit trail
//! POST   /api/ontologies/:id/audit/verify  Integrity check
//! GET    /api/ontologies/:id/audit/stats   User stats
//! POST   /api/ontologies/:id/sam      Run SAM analysis
//! GET    /api/ontologies/:id/sam      List SAM results
//! GET    /api/health                  Health check

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::{self, Db};
use crate::engine;
use crate::models::*;
use crate::service::{self, ServiceCtx};

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/ontologies", post(create_ontology).get(list_ontologies))
        .route("/api/ontologies/{id}", get(get_ontology).delete(del_ontology))
        .route("/api/ontologies/{id}/validate", post(validate))
        .route("/api/ontologies/{id}/ifd", get(ifd_matrix))
        .route("/api/ontologies/{id}/caps", get(cap_matrix))
        .route("/api/ontologies/{id}/simulate", post(simulate))
        .route("/api/ontologies/{id}/runs", get(list_runs))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/ontologies/{id}/audit", get(audit_trail))
        .route("/api/ontologies/{id}/audit/verify", post(verify_audit))
        .route("/api/ontologies/{id}/audit/stats", get(audit_stats))
        .route("/api/ontologies/{id}/sam", post(run_sam).get(list_sam))
        .route("/api/plan/{domain}", post(plan_domain))
        .with_state(state)
}

fn ok(data: Value) -> Json<Value> {
    Json(json!({"ok": true, "data": data}))
}
fn err(msg: &str, _status: u16) -> Json<Value> {
    Json(json!({"ok": false, "error": msg}))
}

// ── Health ───────────────────────────────────────────────────

async fn health(State(st): State<AppState>) -> Json<Value> {
    let _ = st.db.lock().unwrap();
    ok(json!({"status": "ok", "db": "sqlite"}))
}

// ── Ontology CRUD ────────────────────────────────────────────

async fn create_ontology(
    State(st): State<AppState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let spec_val = body.get("spec").unwrap_or(&body);
    let onto: BusinessOntology = match serde_json::from_value(spec_val.clone()) {
        Ok(o) => o,
        Err(e) => return err(&e.to_string(), 400),
    };
    let id = uuid::Uuid::new_v4().to_string();
    match db::insert_ontology(&st.db, &id, &onto) {
        Ok(_) => ok(json!({"id": id, "fingerprint": onto.fingerprint()})),
        Err(e) => err(&e.to_string(), 400),
    }
}

async fn list_ontologies(State(st): State<AppState>) -> Json<Value> {
    match db::list_ontologies(&st.db) {
        Ok(list) => ok(json!(list)),
        Err(e) => err(&e.to_string(), 500),
    }
}

async fn get_ontology(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    match db::get_ontology_full(&st.db, &id) {
        Ok(Some(v)) => ok(v),
        Ok(None) => err("Not found", 404),
        Err(e) => err(&e.to_string(), 500),
    }
}

async fn del_ontology(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    match db::delete_ontology(&st.db, &id) {
        Ok(true) => ok(json!({"deleted": id})),
        Ok(false) => err("Not found", 404),
        Err(e) => err(&e.to_string(), 500),
    }
}

// ── Validation ───────────────────────────────────────────────

async fn validate(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let ctx = ServiceCtx::new(st.db.clone());
    match service::validate(&ctx, &id) {
        Ok(report) => ok(serde_json::to_value(&report).unwrap()),
        Err(e) => err(&e, 400),
    }
}

// ── IFD Matrix ───────────────────────────────────────────────

async fn ifd_matrix(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let onto = match db::get_ontology(&st.db, &id) {
        Ok(Some(o)) => o,
        Ok(None) => return err("Not found", 404),
        Err(e) => return err(&e.to_string(), 500),
    };
    let mut matrix = Vec::new();
    let mut actions: Vec<_> = onto.actions.iter().collect();
    actions.sort_by(|a, b| b.1.ifd_score().cmp(&a.1.ifd_score()));
    for (name, adef) in actions {
        matrix.push(json!({
            "action_name": name,
            "ifd_intensity": adef.intensity.label(),
            "ifd_frequency": adef.frequency.label(),
            "ifd_density": adef.density.label(),
            "ifd_score": adef.ifd_score(),
            "priority": adef.ifd_priority(),
            "customer_impact": adef.customer_impact,
        }));
    }
    ok(json!(matrix))
}

// ── Capability Matrix ────────────────────────────────────────

async fn cap_matrix(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let onto = match db::get_ontology(&st.db, &id) {
        Ok(Some(o)) => o,
        Ok(None) => return err("Not found", 404),
        Err(e) => return err(&e.to_string(), 500),
    };
    let token_map = onto.token_map();
    let mut results = Vec::new();
    for (user, tokens) in &token_map {
        for tok in tokens {
            let enabled: Vec<_> = onto.actions.iter()
                .filter(|(_, a)| &a.required_capability == tok)
                .map(|(name, _)| name.clone())
                .collect();
            results.push(json!({
                "user_id": user,
                "token": tok,
                "enabled_actions": enabled,
            }));
        }
    }
    results.sort_by(|a, b| {
        a["user_id"].as_str().cmp(&b["user_id"].as_str())
    });
    ok(json!(results))
}

// ── Simulation ───────────────────────────────────────────────

#[derive(Deserialize)]
struct SimulateBody {
    actions: Vec<Vec<String>>, // [["user","action","target"],...]
    #[serde(default)]
    label: String,
}

async fn simulate(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SimulateBody>,
) -> Json<Value> {
    let steps: Vec<ActionStep> = body.actions.iter().filter_map(|a| {
        if a.len() >= 3 {
            Some(ActionStep { user: a[0].clone(), action: a[1].clone(), target: a[2].clone() })
        } else { None }
    }).collect();

    // Route through service layer — writes run + audit to SQLite
    let mut ctx = ServiceCtx::new(st.db.clone());
    match service::simulate(&mut ctx, &id, &steps, &body.label) {
        Ok(result) => ok(serde_json::to_value(&result).unwrap()),
        Err(e) => err(&e, 400),
    }
}

async fn list_runs(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    match db::list_runs(&st.db, &id) {
        Ok(list) => ok(json!(list)),
        Err(e) => err(&e.to_string(), 500),
    }
}

async fn get_run(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    match db::get_run(&st.db, &id) {
        Ok(Some(v)) => ok(v),
        Ok(None) => err("Not found", 404),
        Err(e) => err(&e.to_string(), 500),
    }
}

// ── Audit ────────────────────────────────────────────────────

async fn audit_trail(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    match db::list_audit(&st.db, &id, 100) {
        Ok(list) => ok(json!(list)),
        Err(e) => err(&e.to_string(), 500),
    }
}

async fn verify_audit(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let ctx = ServiceCtx::new(st.db.clone());
    match service::verify_audit(&ctx, &id) {
        Ok((valid, count)) => ok(json!({"valid": valid, "entries_checked": count})),
        Err(e) => err(&e, 500),
    }
}

async fn audit_stats(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let ctx = ServiceCtx::new(st.db.clone());
    match service::audit_stats(&ctx, &id) {
        Ok((stats, patterns)) => ok(json!({"user_stats": stats, "failure_patterns": patterns})),
        Err(e) => err(&e, 500),
    }
}

// ── SAM ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SamBody {
    actions: Vec<Vec<String>>,
    #[serde(default = "default_bootstrap")]
    n_bootstrap: usize,
}
fn default_bootstrap() -> usize { 500 }

async fn run_sam(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SamBody>,
) -> Json<Value> {
    let steps: Vec<ActionStep> = body.actions.iter().filter_map(|a| {
        if a.len() >= 3 {
            Some(ActionStep { user: a[0].clone(), action: a[1].clone(), target: a[2].clone() })
        } else { None }
    }).collect();

    let ctx = ServiceCtx::new(st.db.clone());
    match service::run_sam_analysis(&ctx, &id, &steps, body.n_bootstrap) {
        Ok(result) => ok(serde_json::to_value(&result).unwrap()),
        Err(e) => err(&e, 400),
    }
}

// ── Plan Domain (new — exposes planner via API) ──────────────

#[derive(Deserialize)]
struct PlanBody {
    #[serde(default = "default_steps")]
    steps: usize,
}
fn default_steps() -> usize { 6 }

async fn plan_domain(
    State(st): State<AppState>,
    Path(domain): Path<String>,
    Json(body): Json<PlanBody>,
) -> Json<Value> {
    let mut ctx = ServiceCtx::new(st.db.clone());
    match service::plan_domain(&mut ctx, &domain, body.steps) {
        Ok(result) => ok(serde_json::to_value(&result).unwrap()),
        Err(e) => err(&e, 400),
    }
}

async fn list_sam(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    match db::list_sam(&st.db, &id) {
        Ok(list) => ok(json!(list)),
        Err(e) => err(&e.to_string(), 500),
    }
}
