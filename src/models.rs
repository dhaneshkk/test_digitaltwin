//! Structured ontology data model — typed Rust replaces raw ASP strings.
//! MIT principle: "Workflow redesign, not tool overlay."
//! Arity/type bugs are structurally impossible with these types.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

// ══════════════════════════════════════════════════════════════
// IFD Framework (Furr & Shipilov)
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum IfdScore {
    Low = 1,
    Medium = 2,
    High = 3,
}

impl IfdScore {
    pub fn value(self) -> u8 {
        self as u8
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Core Ontology Types
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDef {
    pub input_state: BTreeMap<String, String>,
    pub output_state: BTreeMap<String, String>,
    pub required_capability: String,
    #[serde(default)]
    pub customer_impact: String,
    #[serde(default = "default_ifd")]
    pub intensity: IfdScore,
    #[serde(default = "default_ifd")]
    pub frequency: IfdScore,
    #[serde(default = "default_ifd")]
    pub density: IfdScore,
}

fn default_ifd() -> IfdScore { IfdScore::Medium }

impl ActionDef {
    pub fn ifd_score(&self) -> u8 {
        self.intensity.value() + self.frequency.value() + self.density.value()
    }
    pub fn ifd_priority(&self) -> &'static str {
        match self.ifd_score() {
            8..=9 => "CRITICAL",
            6..=7 => "HIGH",
            _ => "LOW",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyDef {
    pub allowed_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    pub entity_id: String,
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub user_id: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessOntology {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_timesteps")]
    pub max_timesteps: usize,
    pub concepts: Vec<String>,
    pub properties: BTreeMap<String, PropertyDef>,
    pub actions: BTreeMap<String, ActionDef>,
    pub initial_entities: Vec<EntityState>,
    pub capability_grants: Vec<CapabilityGrant>,
}

fn default_version() -> String { "1.0.0".into() }
fn default_timesteps() -> usize { 10 }

impl BusinessOntology {
    /// SHA-256 fingerprint for change detection
    pub fn fingerprint(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        let hash = Sha256::digest(json.as_bytes());
        hex::encode(&hash[..8])
    }

    /// All unique user IDs
    pub fn users(&self) -> BTreeSet<String> {
        self.capability_grants.iter().map(|g| g.user_id.clone()).collect()
    }

    /// All entity IDs
    pub fn entity_ids(&self) -> BTreeSet<String> {
        self.initial_entities.iter().map(|e| e.entity_id.clone()).collect()
    }

    /// Action names
    pub fn action_names(&self) -> Vec<String> {
        self.actions.keys().cloned().collect()
    }

    /// Token → user mapping (user_id → set of tokens)
    pub fn token_map(&self) -> HashMap<String, HashSet<String>> {
        let mut map: HashMap<String, HashSet<String>> = HashMap::new();
        for g in &self.capability_grants {
            map.entry(g.user_id.clone()).or_default().insert(g.token.clone());
        }
        map
    }

    /// Capability → users mapping (token → vec of user_ids)
    pub fn capability_map(&self) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for g in &self.capability_grants {
            map.entry(g.token.clone()).or_default().push(g.user_id.clone());
        }
        map
    }

    /// Convert to OntologyGraph nodes + edges for domain routing
    pub fn to_graph(&self) -> crate::ontology_graph::OntologyGraph {
        let mut g = crate::ontology_graph::OntologyGraph::new();
        for entity in &self.initial_entities {
            let props: Vec<(&str, &str)> = entity.properties.iter()
                .map(|(k, v)| (k.as_str(), v.as_str())).collect();
            g.add(crate::ontology_graph::OntologyNode::new(
                &entity.entity_id, "Entity", &props), None);
        }
        g
    }
}

// ══════════════════════════════════════════════════════════════
// Simulation Results
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionStep {
    pub user: String,
    pub action: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub state: Vec<String>,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub run_id: String,
    pub has_model: bool,
    pub successes: Vec<SuccessEvent>,
    pub errors: Vec<ErrorEvent>,
    pub timeline: BTreeMap<usize, TimelineEntry>,
    pub friction_warnings: Vec<FrictionWarning>,
    pub grounding_us: u64,
    pub solve_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessEvent {
    pub t: usize,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub t: usize,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrictionWarning {
    pub t: usize,
    pub msg: String,
}

// ══════════════════════════════════════════════════════════════
// SAM Analysis
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamResult {
    pub id: String,
    pub mcc: f64,
    pub mcc_ci: (f64, f64),
    pub cohens_kappa: f64,
    pub accuracy: f64,
    pub confusion: ConfusionMatrix,
    pub n_perturbations: usize,
    pub details: Vec<PerturbationDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfusionMatrix {
    pub tp: usize,
    pub tn: usize,
    pub fp: usize,
    pub r#fn: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerturbationDetail {
    pub label: String,
    pub expected_clean: bool,
    pub actual_clean: bool,
}

// ══════════════════════════════════════════════════════════════
// Telemetry (unified — used by planner + SAM classifier + service)
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        serde_json::json!({
            "run_id": self.run_id, "entity_id": self.entity_id,
            "timestep": self.timestep, "n_actions": self.n_actions,
            "n_props": self.n_props, "elapsed_us": self.elapsed_us,
            "label": self.label,
        })
    }

    pub fn features(&self) -> [f64; 4] {
        [self.n_actions as f64, self.n_props as f64,
         self.elapsed_us as f64, self.timestep as f64]
    }
}

// ══════════════════════════════════════════════════════════════
// Planner Result (unified output from forward-chaining planner)
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerResult {
    pub run_id: String,
    pub domain: String,
    pub telemetry: Vec<TelemetryRecord>,
    pub n_entities: usize,
    pub n_distressed: usize,
    pub audit_entries: usize,
    pub chain_valid: bool,
}

// ══════════════════════════════════════════════════════════════
// Validation
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

// ══════════════════════════════════════════════════════════════
// Audit
// ══════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Option<i64>,
    pub run_id: String,
    pub ontology_id: String,
    pub timestep: usize,
    pub user_id: String,
    pub action: String,
    pub target: String,
    pub outcome: String,
    pub detail: String,
    pub chain_hash: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStat {
    pub user_id: String,
    pub total: usize,
    pub successes: usize,
    pub failures: usize,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePattern {
    pub action: String,
    pub outcome: String,
    pub detail: String,
    pub count: usize,
}
