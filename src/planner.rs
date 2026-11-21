//! Forward-chaining planner — executes ontology actions on entities.
//! Produces telemetry records for SAM classifier training.

use crate::iggy::IggyBus;
use crate::models::*;
use chrono::Utc;
use serde_json::json;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone)]
pub struct TelemetryRecord {
    pub run_id: String,
    pub entity_id: String,
    pub timestep: usize,
    pub n_actions: usize,
    pub n_props: usize,
    pub elapsed_us: u64,
    pub label: i32,  // 0 = ok, 1 = distressed
}

impl TelemetryRecord {
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "run_id": self.run_id, "entity_id": self.entity_id,
            "timestep": self.timestep, "n_actions": self.n_actions,
            "n_props": self.n_props, "elapsed_us": self.elapsed_us,
            "label": self.label,
        })
    }

    pub fn features(&self) -> [f64; 4] {
        [
            self.n_actions as f64,
            self.n_props as f64,
            self.elapsed_us as f64,
            self.timestep as f64,
        ]
    }
}

/// Rail fleet ontology with depart/arrive/clear_delay/emergency_stop
pub fn build_rail_ontology(max_steps: usize) -> BusinessOntology {
    let kv = |pairs: &[(&str, &str)]| -> std::collections::BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    };
    let mut actions = std::collections::BTreeMap::new();
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
        properties: std::collections::BTreeMap::new(),  // rail uses inline state
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

/// Run forward-chaining planner on a single entity
pub fn run_planner(
    ont: &BusinessOntology,
    entity: &EntityState,
    registry: &mut crate::engine::GovernanceRegistry,
    bus: &mut IggyBus,
    run_id: &str,
) -> Vec<TelemetryRecord> {
    let mut cap_map: HashMap<String, Vec<String>> = HashMap::new();
    for g in &ont.capability_grants {
        cap_map.entry(g.token.clone()).or_default().push(g.user_id.clone());
    }

    let mut state = entity.properties.clone();
    let mut telemetry = vec![];

    for step in 0..ont.max_timesteps {
        let t0 = std::time::Instant::now();
        let mut fired: Option<(String, std::collections::BTreeMap<String, String>, String)> = None;

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
            bus.emit_audit(entry.to_json());
            let rec = TelemetryRecord {
                run_id: run_id.into(), entity_id: entity.entity_id.clone(),
                timestep: step + 1, n_actions: 1, n_props: state.len(),
                elapsed_us: elapsed, label: 0,
            };
            bus.emit_telem(rec.to_json());
            telemetry.push(rec);
        } else {
            let rec = TelemetryRecord {
                run_id: run_id.into(), entity_id: entity.entity_id.clone(),
                timestep: step + 1, n_actions: 0, n_props: state.len(),
                elapsed_us: elapsed, label: 1,
            };
            bus.emit_telem(rec.to_json());
            telemetry.push(rec);
            break;
        }
    }

    telemetry
}
