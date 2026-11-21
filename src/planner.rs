//! Forward-chaining planner — executes ontology actions on entities.
//! Uses unified models::TelemetryRecord and models::BusinessOntology.
//! No duplicate types — single source of truth in models.rs.

use crate::engine::GovernanceRegistry;
use crate::iggy::IggyBus;
use crate::models::*;
use std::collections::BTreeMap;

/// Rail fleet ontology with depart/arrive/clear_delay/emergency_stop.
/// Returns the canonical BusinessOntology from models.rs.
pub fn build_rail_ontology(max_steps: usize) -> BusinessOntology {
    let kv = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    };
    let mut actions = BTreeMap::new();
    actions.insert("depart".into(), ActionDef {
        input_state: kv(&[("status", "idle"), ("location", "platform")]),
        output_state: kv(&[("status", "running"), ("location", "en_route"), ("speed", "normal")]),
        required_capability: "driver".into(),
        customer_impact: "Initiates scheduled service departure".into(),
        intensity: IfdScore::High, frequency: IfdScore::High, density: IfdScore::Medium,
    });
    actions.insert("arrive".into(), ActionDef {
        input_state: kv(&[("status", "running"), ("location", "en_route")]),
        output_state: kv(&[("status", "idle"), ("location", "platform"), ("speed", "stopped")]),
        required_capability: "driver".into(),
        customer_impact: "Completes journey, enables passenger deboarding".into(),
        intensity: IfdScore::High, frequency: IfdScore::High, density: IfdScore::Medium,
    });
    actions.insert("clear_delay".into(), ActionDef {
        input_state: kv(&[("status", "delayed")]),
        output_state: kv(&[("status", "running"), ("speed", "slow")]),
        required_capability: "controller".into(),
        customer_impact: "Resumes delayed service at reduced speed".into(),
        intensity: IfdScore::Medium, frequency: IfdScore::Low, density: IfdScore::Medium,
    });
    actions.insert("emergency_stop".into(), ActionDef {
        input_state: kv(&[("status", "running")]),
        output_state: kv(&[("status", "delayed"), ("speed", "stopped")]),
        required_capability: "safety_officer".into(),
        customer_impact: "Prevents accidents, protects passengers".into(),
        intensity: IfdScore::High, frequency: IfdScore::Low, density: IfdScore::High,
    });

    BusinessOntology {
        name: "RailFleet".into(),
        version: "4.0.0".into(),
        max_timesteps: max_steps,
        concepts: vec!["train".into(), "station".into(), "controller".into()],
        properties: BTreeMap::new(),
        actions,
        initial_entities: vec![
            EntityState { entity_id: "train_a".into(), properties: kv(&[("status", "idle"), ("location", "platform"), ("speed", "stopped")]) },
            EntityState { entity_id: "train_b".into(), properties: kv(&[("status", "running"), ("location", "en_route"), ("speed", "normal")]) },
            EntityState { entity_id: "train_c".into(), properties: kv(&[("status", "delayed"), ("location", "en_route"), ("speed", "stopped")]) },
        ],
        capability_grants: vec![
            CapabilityGrant { user_id: "op_alice".into(), token: "driver".into() },
            CapabilityGrant { user_id: "op_alice".into(), token: "safety_officer".into() },
            CapabilityGrant { user_id: "op_bob".into(), token: "driver".into() },
            CapabilityGrant { user_id: "op_carol".into(), token: "controller".into() },
        ],
    }
}

/// Run forward-chaining planner on a single entity.
/// Uses models::TelemetryRecord (unified type).
/// Optionally writes to IggyBus if provided.
pub fn run_planner(
    ont: &BusinessOntology,
    entity: &EntityState,
    registry: &mut GovernanceRegistry,
    bus: Option<&mut IggyBus>,
    run_id: &str,
) -> Vec<TelemetryRecord> {
    let cap_map = ont.capability_map();
    let mut state = entity.properties.clone();
    let mut telemetry = vec![];

    for step in 0..ont.max_timesteps {
        let t0 = std::time::Instant::now();
        let mut fired: Option<(String, BTreeMap<String, String>, String)> = None;

        'outer: for (aname, adef) in &ont.actions {
            let users = cap_map.get(&adef.required_capability).cloned().unwrap_or_default();
            if users.is_empty() { continue; }
            for (prop, val) in &adef.input_state {
                if state.get(prop).map(|v| v.as_str()) != Some(val.as_str()) {
                    continue 'outer;
                }
            }
            fired = Some((aname.clone(), adef.output_state.clone(), users[0].clone()));
            break;
        }

        let elapsed = t0.elapsed().as_micros() as u64;

        if let Some((aname, effects, user)) = fired {
            for (k, v) in &effects {
                state.insert(k.clone(), v.clone());
            }
            let entry = registry.record(run_id, step + 1, &user, &aname, &entity.entity_id, "EXECUTED", "");
            if let Some(b) = bus.as_deref_mut() {
                b.emit_audit(entry.to_json());
            }
            let rec = TelemetryRecord {
                run_id: run_id.into(), entity_id: entity.entity_id.clone(),
                timestep: step + 1, n_actions: 1, n_props: state.len(),
                elapsed_us: elapsed, label: 0,
            };
            if let Some(b) = bus.as_deref_mut() {
                b.emit_telem(rec.to_json());
            }
            telemetry.push(rec);
        } else {
            let rec = TelemetryRecord {
                run_id: run_id.into(), entity_id: entity.entity_id.clone(),
                timestep: step + 1, n_actions: 0, n_props: state.len(),
                elapsed_us: elapsed, label: 1,
            };
            if let Some(b) = bus.as_deref_mut() {
                b.emit_telem(rec.to_json());
            }
            telemetry.push(rec);
            break;
        }
    }

    telemetry
}
