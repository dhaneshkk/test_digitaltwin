//! Pure Rust simulation engine — deterministic state machine.
//! Replaces ASP/Clingo with direct state-transition evaluation.
//! MIT: "Engineer FOR friction, don't eliminate it."
//! HBR: "Connect to customer value, design for scale from day one."

use crate::models::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;
use uuid::Uuid;

// ══════════════════════════════════════════════════════════════
// Ontology Validator (Design for Scale — catch errors early)
// ══════════════════════════════════════════════════════════════

pub fn validate_ontology(onto: &BusinessOntology) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // 1. Property references
    for (aname, adef) in &onto.actions {
        for (state, label) in [(&adef.input_state, "input"), (&adef.output_state, "output")] {
            for (prop, val) in state {
                match onto.properties.get(prop) {
                    None => errors.push(format!(
                        "Action '{aname}' {label}: undefined property '{prop}'"
                    )),
                    Some(pdef) if !pdef.allowed_values.contains(val) => errors.push(format!(
                        "Action '{aname}' {label}: '{val}' not in {:?}", pdef.allowed_values
                    )),
                    _ => {}
                }
            }
        }
    }

    // 2. Capability coverage
    let required: HashSet<_> = onto.actions.values().map(|a| &a.required_capability).collect();
    let granted: HashSet<_> = onto.capability_grants.iter().map(|g| &g.token).collect();
    for tok in &required {
        if !granted.contains(*tok) {
            errors.push(format!("Capability '{tok}' required but never granted"));
        }
    }

    // 3. Customer value linkage (HBR)
    for (name, adef) in &onto.actions {
        if adef.customer_impact.trim().is_empty() {
            warnings.push(format!("Action '{name}': no customer_impact (HBR/IFD trap)"));
        }
    }

    // 4. Initial state consistency
    for entity in &onto.initial_entities {
        for (prop, val) in &entity.properties {
            match onto.properties.get(prop) {
                None => errors.push(format!(
                    "Entity '{}': undefined property '{prop}'", entity.entity_id
                )),
                Some(pdef) if !pdef.allowed_values.contains(val) => errors.push(format!(
                    "Entity '{}': invalid value '{val}' for '{prop}'", entity.entity_id
                )),
                _ => {}
            }
        }
    }

    // 5. Reachability
    let mut reachable: HashMap<String, HashSet<String>> = HashMap::new();
    for ent in &onto.initial_entities {
        for (p, v) in &ent.properties {
            reachable.entry(p.clone()).or_default().insert(v.clone());
        }
    }
    for adef in onto.actions.values() {
        for (p, v) in &adef.output_state {
            reachable.entry(p.clone()).or_default().insert(v.clone());
        }
    }
    for (name, adef) in &onto.actions {
        for (p, v) in &adef.input_state {
            if !reachable.get(p).map_or(false, |s| s.contains(v)) {
                warnings.push(format!(
                    "Action '{name}' requires {p}={v} but no source produces it"
                ));
            }
        }
    }

    // 6. Deadlock detection
    for (aname, adef) in &onto.actions {
        for (prop, out_val) in &adef.output_state {
            let can_leave = onto.actions.values().any(|other| {
                other.input_state.get(prop) == Some(out_val)
                    && other.output_state.get(prop).map_or(false, |v| v != out_val)
            });
            if !can_leave {
                let used_as_input = onto.actions.values().any(|a| {
                    a.input_state.get(prop) == Some(out_val)
                });
                if !used_as_input {
                    warnings.push(format!(
                        "Terminal state: {prop}={out_val} (from '{aname}') has no outgoing transition"
                    ));
                }
            }
        }
    }

    // 7. Token economy — over-privileged users
    let total_actions = onto.actions.len();
    let mut user_token_count: HashMap<String, usize> = HashMap::new();
    for g in &onto.capability_grants {
        *user_token_count.entry(g.user_id.clone()).or_default() += 1;
    }
    for (user, count) in &user_token_count {
        if total_actions > 2 && *count as f64 / total_actions as f64 > 0.8 {
            warnings.push(format!(
                "User '{user}' holds {count}/{total_actions} tokens — over-privileged"
            ));
        }
    }

    ValidationReport {
        is_valid: errors.is_empty(),
        errors,
        warnings,
    }
}

// ══════════════════════════════════════════════════════════════
// Friction Layer — Stateful Pre-flight (PROD-05)
// MIT: "Friction tells you where judgment matters."
// ══════════════════════════════════════════════════════════════

pub fn friction_check(
    onto: &BusinessOntology,
    actions: &[ActionStep],
) -> Vec<FrictionWarning> {
    let mut warnings = Vec::new();
    let token_map = onto.token_map();
    let entity_ids = onto.entity_ids();

    // Build simulated state per entity
    let mut sim_state: HashMap<String, BTreeMap<String, String>> = HashMap::new();
    for ent in &onto.initial_entities {
        sim_state.insert(ent.entity_id.clone(), ent.properties.clone());
    }

    for (i, step) in actions.iter().enumerate() {
        let adef = match onto.actions.get(&step.action) {
            Some(a) => a,
            None => {
                warnings.push(FrictionWarning {
                    t: i,
                    msg: format!("Action '{}' not in ontology", step.action),
                });
                continue;
            }
        };

        if !entity_ids.contains(&step.target) {
            warnings.push(FrictionWarning {
                t: i,
                msg: format!("Entity '{}' not found", step.target),
            });
            continue;
        }

        let user_tokens = token_map.get(&step.user);
        if !user_tokens.map_or(false, |t| t.contains(&adef.required_capability)) {
            warnings.push(FrictionWarning {
                t: i,
                msg: format!(
                    "{} lacks '{}' for '{}'",
                    step.user, adef.required_capability, step.action
                ),
            });
            continue;
        }

        // Stateful check: simulate state propagation
        let current = sim_state.get(&step.target);
        let mut state_ok = true;
        for (prop, req_val) in &adef.input_state {
            let actual = current.and_then(|s| s.get(prop));
            if actual != Some(req_val) {
                warnings.push(FrictionWarning {
                    t: i,
                    msg: format!(
                        "{prop} is '{}', needs '{req_val}'",
                        actual.unwrap_or(&"(unset)".to_string())
                    ),
                });
                state_ok = false;
            }
        }

        // If preconditions met, apply transition
        if state_ok {
            if let Some(state) = sim_state.get_mut(&step.target) {
                for (p, v) in &adef.output_state {
                    state.insert(p.clone(), v.clone());
                }
            }
        }
    }

    warnings
}

// ══════════════════════════════════════════════════════════════
// Pure Rust Simulation Engine (replaces ASP/Clingo)
// Deterministic state-machine with frame axiom (inertia)
// ══════════════════════════════════════════════════════════════

pub fn simulate(
    onto: &BusinessOntology,
    actions: &[ActionStep],
) -> SimulationResult {
    let run_id = Uuid::new_v4().to_string();
    let token_map = onto.token_map();

    // Build initial world state: entity_id → { prop → val }
    let mut world: HashMap<String, BTreeMap<String, String>> = HashMap::new();
    for ent in &onto.initial_entities {
        world.insert(ent.entity_id.clone(), ent.properties.clone());
    }

    let mut timeline: BTreeMap<usize, TimelineEntry> = BTreeMap::new();
    let mut successes = Vec::new();
    let mut errors = Vec::new();

    // Record initial state at T=0 if no action at T=0
    // (we always record state snapshots)

    let t0 = Instant::now();

    // Friction pre-flight
    let friction_warnings = friction_check(onto, actions);

    let grounding_us = t0.elapsed().as_micros() as u64;
    let t1 = Instant::now();

    // Execute each action step
    for (t, step) in actions.iter().enumerate() {
        let mut entry = TimelineEntry {
            state: Vec::new(),
            events: Vec::new(),
        };

        let adef = match onto.actions.get(&step.action) {
            Some(a) => a,
            None => {
                let detail = format!("unknown action '{}'", step.action);
                entry.events.push(format!("❌ {detail}"));
                errors.push(ErrorEvent { t, detail });
                timeline.insert(t, entry);
                continue;
            }
        };

        // Check capability
        let has_cap = token_map
            .get(&step.user)
            .map_or(false, |tokens| tokens.contains(&adef.required_capability));

        if !has_cap {
            let detail = format!("missing cap {}", adef.required_capability);
            entry.events.push(format!("❌ {detail}"));
            errors.push(ErrorEvent { t, detail });
            // Still record state snapshot
            if let Some(state) = world.get(&step.target) {
                for (p, v) in state {
                    entry.state.push(format!("{}.{p} = {v}", step.target));
                }
            }
            timeline.insert(t, entry);
            continue;
        }

        // Check input state preconditions
        let entity_state = world.get(&step.target);
        let mut preconditions_met = true;
        for (prop, req_val) in &adef.input_state {
            let actual = entity_state.and_then(|s| s.get(prop));
            if actual != Some(req_val) {
                let detail = format!("bad state {prop} {req_val}");
                entry.events.push(format!("❌ {detail}"));
                errors.push(ErrorEvent {
                    t,
                    detail: detail.clone(),
                });
                preconditions_met = false;
            }
        }

        if preconditions_met {
            // Apply state transition (output_state overwrites, frame axiom preserves rest)
            if let Some(state) = world.get_mut(&step.target) {
                for (p, v) in &adef.output_state {
                    state.insert(p.clone(), v.clone());
                }
            }
            entry.events.push(format!("✅ {}", step.action));
            successes.push(SuccessEvent {
                t,
                action: step.action.clone(),
            });
        }

        // Record state snapshot at this timestep
        if let Some(state) = world.get(&step.target) {
            for (p, v) in state {
                entry.state.push(format!("{}.{p} = {v}", step.target));
            }
        }

        timeline.insert(t, entry);
    }

    // Record final propagated state for all entities at T = actions.len()
    if !actions.is_empty() {
        let final_t = actions.len();
        let mut final_entry = TimelineEntry {
            state: Vec::new(),
            events: Vec::new(),
        };
        for (eid, state) in &world {
            for (p, v) in state {
                final_entry.state.push(format!("{eid}.{p} = {v}"));
            }
        }
        final_entry.state.sort();
        timeline.insert(final_t, final_entry);
    }

    let solve_us = t1.elapsed().as_micros() as u64;

    SimulationResult {
        run_id,
        has_model: !actions.is_empty() || onto.initial_entities.is_empty(),
        successes,
        errors,
        timeline,
        friction_warnings,
        grounding_us,
        solve_us,
    }
}

// ══════════════════════════════════════════════════════════════
// SHA-256 Audit Hash Chain (PROD-04)
// ══════════════════════════════════════════════════════════════

pub fn compute_chain_hash(
    timestep: usize,
    user: &str,
    action: &str,
    target: &str,
    outcome: &str,
    detail: &str,
    prev_hash: &str,
) -> String {
    let payload = format!("{prev_hash}|{timestep}|{user}|{action}|{target}|{outcome}|{detail}");
    let hash = Sha256::digest(payload.as_bytes());
    hex::encode(&hash[..16])
}

// ══════════════════════════════════════════════════════════════
// SAM Robustness Analysis (PROD-07/08)
// ══════════════════════════════════════════════════════════════

pub fn sam_analysis(
    onto: &BusinessOntology,
    base_actions: &[ActionStep],
    n_bootstrap: usize,
) -> SamResult {
    // Generate perturbations
    let mut perturbations: Vec<(String, Vec<ActionStep>, bool)> = Vec::new();

    perturbations.push(("Baseline".into(), base_actions.to_vec(), true));

    if !base_actions.is_empty() {
        // Reversed
        let mut rev = base_actions.to_vec();
        rev.reverse();
        perturbations.push(("Reversed".into(), rev, false));

        // Per-position perturbations
        for i in 0..base_actions.len() {
            // Unknown user
            let mut p = base_actions.to_vec();
            p[i].user = "user_unknown".into();
            perturbations.push((format!("Unknown user T={i}"), p, false));

            // Bad action
            let mut p = base_actions.to_vec();
            p[i].action = "nonexistent".into();
            perturbations.push((format!("Bad action T={i}"), p, false));

            // Phantom entity
            let mut p = base_actions.to_vec();
            p[i].target = "phantom".into();
            perturbations.push((format!("Phantom entity T={i}"), p, false));
        }

        // Empty
        perturbations.push(("Empty".into(), vec![], false));

        // Adjacent swaps
        for i in 0..base_actions.len().saturating_sub(1) {
            let mut p = base_actions.to_vec();
            p.swap(i, i + 1);
            perturbations.push((format!("Swap T={i}↔T={}", i + 1), p, false));
        }
    }

    // Run each perturbation
    let mut details = Vec::new();
    for (label, seq, expected_clean) in &perturbations {
        let result = simulate(onto, seq);
        let actual_clean = result.errors.is_empty() && !seq.is_empty();
        details.push(PerturbationDetail {
            label: label.clone(),
            expected_clean: *expected_clean,
            actual_clean,
        });
    }

    // Confusion matrix
    let (mut tp, mut tn, mut fp, mut r#fn) = (0usize, 0, 0, 0);
    for d in &details {
        match (d.expected_clean, d.actual_clean) {
            (true, true) => tp += 1,
            (false, false) => tn += 1,
            (false, true) => fp += 1,
            (true, false) => r#fn += 1,
        }
    }

    let n = details.len();
    let accuracy = if n > 0 { (tp + tn) as f64 / n as f64 } else { 0.0 };

    let mcc = compute_mcc(tp, tn, fp, r#fn);

    // Bootstrap MCC CI
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut mcc_samples = Vec::with_capacity(n_bootstrap);
    for _ in 0..n_bootstrap {
        let (mut btp, mut btn, mut bfp, mut bfn) = (0, 0, 0, 0);
        for _ in 0..n {
            let idx = rng.gen_range(0..n);
            let d = &details[idx];
            match (d.expected_clean, d.actual_clean) {
                (true, true) => btp += 1,
                (false, false) => btn += 1,
                (false, true) => bfp += 1,
                (true, false) => bfn += 1,
            }
        }
        mcc_samples.push(compute_mcc(btp, btn, bfp, bfn));
    }
    mcc_samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let ci_lo_idx = (n_bootstrap as f64 * 0.025) as usize;
    let ci_hi_idx = (n_bootstrap as f64 * 0.975) as usize;
    let mcc_ci = (
        mcc_samples.get(ci_lo_idx).copied().unwrap_or(0.0),
        mcc_samples.get(ci_hi_idx.min(mcc_samples.len() - 1)).copied().unwrap_or(0.0),
    );

    // Cohen's kappa
    let pe = if n > 0 {
        ((tp + fp) * (tp + r#fn) + (tn + r#fn) * (tn + fp)) as f64 / (n * n) as f64
    } else {
        0.0
    };
    let cohens_kappa = if (1.0 - pe).abs() > f64::EPSILON {
        (accuracy - pe) / (1.0 - pe)
    } else {
        0.0
    };

    SamResult {
        id: Uuid::new_v4().to_string(),
        mcc,
        mcc_ci,
        cohens_kappa,
        accuracy,
        confusion: ConfusionMatrix { tp, tn, fp, r#fn },
        n_perturbations: n,
        details,
    }
}

fn compute_mcc(tp: usize, tn: usize, fp: usize, r#fn: usize) -> f64 {
    let denom = ((tp + fp) as f64 * (tp + r#fn) as f64 * (tn + fp) as f64 * (tn + r#fn) as f64)
        .sqrt();
    if denom > 0.0 {
        ((tp * tn) as f64 - (fp * r#fn) as f64) / denom
    } else {
        0.0
    }
}
