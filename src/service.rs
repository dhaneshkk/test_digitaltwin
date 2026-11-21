//! Shared service layer — single entry point for both CLI and web API.
//!
//! Every operation that mutates state (simulate, plan, SAM, audit)
//! goes through this layer, which writes to SQLite and optionally
//! publishes to Iggy. No more split data paths.
//!
//! CLI calls:  service::plan_domain(ctx, ...) → PlannerResult
//! API calls:  service::simulate(ctx, ...) → SimulationResult
//! Both write to the same SQLite DB and audit chain.

use crate::db::{self, Db};
use crate::domains;
use crate::engine;
use crate::iggy::IggyBus;
use crate::models::*;
use crate::ontology_graph;
use crate::planner;
use crate::sam_classifier;

use serde_json::json;
use uuid::Uuid;

/// Service context — holds DB handle + optional Iggy bus.
/// Both CLI and API construct this and pass it to service functions.
pub struct ServiceCtx {
    pub db: Db,
    pub bus: Option<IggyBus>,
}

impl ServiceCtx {
    pub fn new(db: Db) -> Self {
        Self { db, bus: None }
    }

    pub fn with_iggy(mut self, host: &str, port: u16) -> Self {
        let mut bus = IggyBus::new(host, port);
        bus.connect();
        self.bus = Some(bus);
        self
    }

    fn bus_mut(&mut self) -> Option<&mut IggyBus> {
        self.bus.as_mut()
    }
}

// ══════════════════════════════════════════════════════════════
// Ontology CRUD (shared)
// ══════════════════════════════════════════════════════════════

pub fn create_ontology(ctx: &ServiceCtx, onto: &BusinessOntology) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    db::insert_ontology(&ctx.db, &id, onto).map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn get_ontology(ctx: &ServiceCtx, id: &str) -> Result<BusinessOntology, String> {
    db::get_ontology(&ctx.db, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Ontology '{}' not found", id))
}

pub fn load_or_build_ontology(ctx: &ServiceCtx, domain: &str, steps: usize) -> Result<(String, BusinessOntology), String> {
    if domain == "rail" {
        let ont = planner::build_rail_ontology(steps);
        // Persist to SQLite so web UI can see it too
        let id = match find_ontology_by_name(ctx, &ont.name, &ont.version) {
            Some(existing_id) => existing_id,
            None => create_ontology(ctx, &ont)?,
        };
        Ok((id, ont))
    } else {
        // For domain plugins, build a synthetic ontology from the graph
        let graph = ontology_graph::build_graph(domain);
        let plugin = domains::get_plugin(domain);
        let ont = BusinessOntology {
            name: format!("{} Domain", plugin.name()),
            version: "4.0.0".into(),
            max_timesteps: steps,
            concepts: vec![plugin.source_type().into(), plugin.target_type().into()],
            properties: std::collections::BTreeMap::new(),
            actions: std::collections::BTreeMap::new(),
            initial_entities: graph.all().map(|n| EntityState {
                entity_id: n.node_id.clone(),
                properties: n.properties.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            }).collect(),
            capability_grants: vec![
                CapabilityGrant { user_id: "platform".into(), token: "reroute".into() },
            ],
        };
        let id = match find_ontology_by_name(ctx, &ont.name, &ont.version) {
            Some(existing_id) => existing_id,
            None => create_ontology(ctx, &ont)?,
        };
        Ok((id, ont))
    }
}

fn find_ontology_by_name(ctx: &ServiceCtx, name: &str, version: &str) -> Option<String> {
    let list = db::list_ontologies(&ctx.db).ok()?;
    list.iter().find(|o| {
        o["name"].as_str() == Some(name) && o["version"].as_str() == Some(version)
    }).and_then(|o| o["id"].as_str().map(|s| s.to_string()))
}

// ══════════════════════════════════════════════════════════════
// Simulate (state machine — used by web API)
// Writes run + audit to SQLite, optionally to Iggy.
// ══════════════════════════════════════════════════════════════

pub fn simulate(
    ctx: &mut ServiceCtx,
    ontology_id: &str,
    actions: &[ActionStep],
    label: &str,
) -> Result<SimulationResult, String> {
    let onto = get_ontology(ctx, ontology_id)?;
    let result = engine::simulate(&onto, actions);

    // Persist run to SQLite
    db::insert_run(&ctx.db, &result.run_id, ontology_id, label, &result, actions)
        .map_err(|e| e.to_string())?;

    // Persist audit entries with hash chain to SQLite
    let mut prev_hash = db::get_last_chain_hash(&ctx.db, ontology_id).unwrap_or_default();
    for s in &result.successes {
        if s.t < actions.len() {
            let step = &actions[s.t];
            let h = engine::compute_chain_hash(s.t, &step.user, &s.action, &step.target, "success", "", &prev_hash);
            let entry = AuditEntry {
                id: None, run_id: result.run_id.clone(), ontology_id: ontology_id.into(),
                timestep: s.t, user_id: step.user.clone(), action: s.action.clone(),
                target: step.target.clone(), outcome: "success".into(),
                detail: String::new(), chain_hash: h.clone(), created_at: None,
            };
            let _ = db::insert_audit_entry(&ctx.db, &entry);
            if let Some(bus) = ctx.bus_mut() {
                bus.emit_audit(json!({"timestep": s.t, "user": step.user, "action": s.action, "outcome": "success", "hash": h}));
            }
            prev_hash = h;
        }
    }
    for e in &result.errors {
        if e.t < actions.len() {
            let step = &actions[e.t];
            let outcome = if e.detail.contains("missing") { "error_capability" } else { "error_state" };
            let h = engine::compute_chain_hash(e.t, &step.user, &step.action, &step.target, outcome, &e.detail, &prev_hash);
            let entry = AuditEntry {
                id: None, run_id: result.run_id.clone(), ontology_id: ontology_id.into(),
                timestep: e.t, user_id: step.user.clone(), action: step.action.clone(),
                target: step.target.clone(), outcome: outcome.into(),
                detail: e.detail.clone(), chain_hash: h.clone(), created_at: None,
            };
            let _ = db::insert_audit_entry(&ctx.db, &entry);
            if let Some(bus) = ctx.bus_mut() {
                bus.emit_audit(json!({"timestep": e.t, "user": step.user, "action": step.action, "outcome": outcome, "hash": h}));
            }
            prev_hash = h;
        }
    }

    Ok(result)
}

// ══════════════════════════════════════════════════════════════
// Plan Domain (forward-chaining planner — used by CLI + API)
// Writes telemetry + audit to SQLite AND Iggy.
// ══════════════════════════════════════════════════════════════

pub fn plan_domain(
    ctx: &mut ServiceCtx,
    domain: &str,
    steps: usize,
) -> Result<PlannerResult, String> {
    let run_id = format!("plan_{}", Uuid::new_v4().simple());
    let (ontology_id, ont) = load_or_build_ontology(ctx, domain, steps)?;
    let mut registry = engine::GovernanceRegistry::new();

    let mut all_telem: Vec<TelemetryRecord> = vec![];

    if domain == "rail" {
        for entity in &ont.initial_entities {
            let t = planner::run_planner(&ont, entity, &mut registry, ctx.bus_mut(), &run_id);
            all_telem.extend(t);
        }
    } else {
        let plugin = domains::get_plugin(domain);
        let mut graph = ontology_graph::build_graph(domain);

        let distressed: Vec<String> = graph.all()
            .filter(|n| plugin.is_distressed(n, &graph))
            .map(|n| n.node_id.clone()).collect();
        let available: Vec<String> = graph.all()
            .filter(|n| plugin.is_available(n, &graph))
            .map(|n| n.node_id.clone()).collect();

        let mut suggestions = vec![];
        for src_id in &distressed {
            let cands: Vec<_> = available.iter()
                .filter_map(|id| graph.get(id).cloned()).collect();
            if cands.is_empty() { continue; }
            let src = match graph.get(src_id) { Some(n) => n.clone(), None => continue };
            if let Some((tgt, reason)) = plugin.suggest_target(&src, &cands, &graph) {
                suggestions.push((src_id.clone(), tgt, reason));
            }
        }

        for (src, tgt, reason) in suggestions {
            if graph.reroute(&src, &tgt).is_ok() {
                registry.record(&run_id, 0, "platform", "reroute", &src, "COMMITTED", &tgt);
                if let Some(bus) = ctx.bus_mut() {
                    bus.emit_event(json!({"event": "reroute", "src": src, "tgt": tgt, "reason": reason}));
                }
                all_telem.push(TelemetryRecord {
                    run_id: run_id.clone(), entity_id: src,
                    timestep: 1, n_actions: 1, n_props: 3, elapsed_us: 0, label: 0,
                });
            }
        }
    }

    let n_distressed = all_telem.iter().filter(|r| r.label == 1).count();
    let chain_valid = registry.verify_chain();

    // Persist planner audit to SQLite (bridge CLI → SQLite)
    let mut prev_hash = db::get_last_chain_hash(&ctx.db, &ontology_id).unwrap_or_default();
    for entry in &registry.entries {
        let h = engine::compute_chain_hash(
            entry.timestep, &entry.user, &entry.action, &entry.target,
            &entry.outcome, &entry.detail, &prev_hash,
        );
        let audit = AuditEntry {
            id: None, run_id: run_id.clone(), ontology_id: ontology_id.clone(),
            timestep: entry.timestep, user_id: entry.user.clone(),
            action: entry.action.clone(), target: entry.target.clone(),
            outcome: entry.outcome.clone(), detail: entry.detail.clone(),
            chain_hash: h.clone(), created_at: None,
        };
        let _ = db::insert_audit_entry(&ctx.db, &audit);
        prev_hash = h;
    }

    // Persist the run summary
    let sim_result = SimulationResult {
        run_id: run_id.clone(),
        has_model: true,
        successes: all_telem.iter().filter(|r| r.label == 0).map(|r| SuccessEvent { t: r.timestep, action: "plan".into() }).collect(),
        errors: all_telem.iter().filter(|r| r.label == 1).map(|r| ErrorEvent { t: r.timestep, detail: "distressed".into() }).collect(),
        timeline: std::collections::BTreeMap::new(),
        friction_warnings: vec![],
        grounding_us: 0,
        solve_us: 0,
    };
    let _ = db::insert_run(&ctx.db, &run_id, &ontology_id,
        &format!("{} plan", domain), &sim_result, &[]);

    Ok(PlannerResult {
        run_id,
        domain: domain.into(),
        telemetry: all_telem,
        n_entities: ont.initial_entities.len(),
        n_distressed,
        audit_entries: registry.entries.len(),
        chain_valid,
    })
}

// ══════════════════════════════════════════════════════════════
// SAM Analysis (shared — CLI + API both write to SQLite)
// ══════════════════════════════════════════════════════════════

pub fn run_sam_analysis(
    ctx: &ServiceCtx,
    ontology_id: &str,
    actions: &[ActionStep],
    n_bootstrap: usize,
) -> Result<SamResult, String> {
    let onto = get_ontology(ctx, ontology_id)?;
    let result = engine::sam_analysis(&onto, actions, n_bootstrap);
    db::insert_sam(&ctx.db, ontology_id, actions, &result).map_err(|e| e.to_string())?;
    Ok(result)
}

// ══════════════════════════════════════════════════════════════
// SAM Eval with classifier training (CLI eval subcommand)
// ══════════════════════════════════════════════════════════════

pub struct EvalResult {
    pub mcc: sam_classifier::MCCResult,
    pub loss_curve: Vec<f64>,
    pub n_samples: usize,
}

pub fn run_eval(
    ctx: &mut ServiceCtx,
    domain: &str,
    steps: usize,
    epochs: usize,
    rho: f64,
) -> Result<EvalResult, String> {
    let plan_result = plan_domain(ctx, domain, steps)?;
    let mut telem = plan_result.telemetry;

    // Synthetic data augmentation
    use rand::{Rng, SeedableRng, rngs::StdRng};
    let mut rng = StdRng::seed_from_u64(7);
    for i in 0..120usize {
        let dist = i % 5 == 0;
        telem.push(TelemetryRecord {
            run_id: "synth".into(), entity_id: format!("s{}", i),
            timestep: i + 1,
            n_actions: if dist { 0 } else { rng.gen_range(1..5) },
            n_props: rng.gen_range(2..8),
            elapsed_us: if dist { rng.gen_range(5000..20000) } else { rng.gen_range(10..500) },
            label: if dist { 1 } else { 0 },
        });
    }

    let mut clf = sam_classifier::DistressClassifier::new();
    clf.fit(&telem, epochs, 0.05, rho);
    let mcc = sam_classifier::evaluate_mcc(&clf, &telem);

    Ok(EvalResult {
        mcc,
        loss_curve: clf.loss_curve,
        n_samples: telem.len(),
    })
}

// ══════════════════════════════════════════════════════════════
// Validation (shared)
// ══════════════════════════════════════════════════════════════

pub fn validate(ctx: &ServiceCtx, ontology_id: &str) -> Result<ValidationReport, String> {
    let onto = get_ontology(ctx, ontology_id)?;
    let report = engine::validate_ontology(&onto);
    let _ = db::insert_validation(&ctx.db, ontology_id, &report);
    Ok(report)
}

// ══════════════════════════════════════════════════════════════
// Audit (shared)
// ══════════════════════════════════════════════════════════════

pub fn verify_audit(ctx: &ServiceCtx, ontology_id: &str) -> Result<(bool, usize), String> {
    db::verify_audit_chain(&ctx.db, ontology_id).map_err(|e| e.to_string())
}

pub fn audit_stats(ctx: &ServiceCtx, ontology_id: &str) -> Result<(Vec<UserStat>, Vec<FailurePattern>), String> {
    let stats = db::user_stats(&ctx.db, ontology_id).unwrap_or_default();
    let patterns = db::failure_patterns(&ctx.db, ontology_id).unwrap_or_default();
    Ok((stats, patterns))
}

// ══════════════════════════════════════════════════════════════
// Q&A (shared — wraps qa module)
// ══════════════════════════════════════════════════════════════

pub fn operator_qa(
    ctx: &mut ServiceCtx,
    domain: &str,
    steps: usize,
    question: &str,
    api_key: &str,
) -> Result<String, String> {
    let plan = plan_domain(ctx, domain, steps)?;
    let registry = engine::GovernanceRegistry::new(); // read-only for QA context
    let bus_ref = ctx.bus.as_ref();
    // Build a temporary registry from DB audit entries for context
    let answer = if let Some(bus) = bus_ref {
        crate::qa::operator_qa(question, &registry, &plan.telemetry, bus, api_key)
    } else {
        let dummy_bus = IggyBus::new("127.0.0.1", 3000);
        crate::qa::operator_qa(question, &registry, &plan.telemetry, &dummy_bus, api_key)
    };
    Ok(answer)
}
