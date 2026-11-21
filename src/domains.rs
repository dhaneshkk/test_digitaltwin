//! Domain plugins — pluggable rerouting strategies per vertical.
//! Each plugin defines: source/target types, distress detection,
//! availability criteria, and target suggestion logic.

use crate::ontology_graph::{OntologyGraph, OntologyNode};

pub trait DomainPlugin {
    fn name(&self) -> &str;
    fn source_type(&self) -> &str;
    fn target_type(&self) -> &str;
    fn description(&self) -> &str;
    fn is_distressed(&self, n: &OntologyNode, g: &OntologyGraph) -> bool;
    fn is_available(&self, n: &OntologyNode, g: &OntologyGraph) -> bool;
    fn suggest_target(
        &self,
        src: &OntologyNode,
        candidates: &[OntologyNode],
        g: &OntologyGraph,
    ) -> Option<(String, String)>;
}

// ── Logistics ────────────────────────────────────────────────

pub struct LogisticsPlugin;
impl DomainPlugin for LogisticsPlugin {
    fn name(&self) -> &str { "Logistics" }
    fn source_type(&self) -> &str { "Shipment" }
    fn target_type(&self) -> &str { "Warehouse" }
    fn description(&self) -> &str { "Delayed shipment → lowest-capacity warehouse rerouting" }
    fn is_distressed(&self, n: &OntologyNode, _g: &OntologyGraph) -> bool {
        n.node_type == "Shipment" && n.get("status") == "DELAYED"
    }
    fn is_available(&self, n: &OntologyNode, _g: &OntologyGraph) -> bool {
        n.node_type == "Warehouse" && n.get_f64("capacity_percent") < 60.0
    }
    fn suggest_target(&self, src: &OntologyNode, candidates: &[OntologyNode], _g: &OntologyGraph) -> Option<(String, String)> {
        let needs_cold = src.properties.contains_key("cargo_temp_c") && src.get_f64("cargo_temp_c") < 0.0;
        candidates.iter()
            .filter(|n| !needs_cold || n.get("has_cold_storage") == "true")
            .min_by_key(|n| (n.get_f64("capacity_percent") * 100.0) as i64)
            .map(|n| (n.node_id.clone(), format!("lowest cap={}%", n.get("capacity_percent"))))
    }
}

// ── Incident ─────────────────────────────────────────────────

pub struct IncidentPlugin;
impl DomainPlugin for IncidentPlugin {
    fn name(&self) -> &str { "Incident" }
    fn source_type(&self) -> &str { "Incident" }
    fn target_type(&self) -> &str { "Service" }
    fn description(&self) -> &str { "P1 open incidents → least-loaded service reassignment" }
    fn is_distressed(&self, n: &OntologyNode, _g: &OntologyGraph) -> bool {
        n.node_type == "Incident" && n.get("status") == "OPEN" && n.get("priority") == "P1"
    }
    fn is_available(&self, n: &OntologyNode, _g: &OntologyGraph) -> bool {
        n.node_type == "Service" && n.get_f64("cpu_usage") < 70.0 && n.get_i64("incident_count") < 3
    }
    fn suggest_target(&self, _src: &OntologyNode, candidates: &[OntologyNode], _g: &OntologyGraph) -> Option<(String, String)> {
        candidates.iter()
            .min_by_key(|n| (n.get_f64("cpu_usage") as i64) * 10 + n.get_i64("incident_count") * 50)
            .map(|n| (n.node_id.clone(), format!("cpu={}% count={}", n.get("cpu_usage"), n.get("incident_count"))))
    }
}

// ── Hospital ─────────────────────────────────────────────────

pub struct HospitalPlugin;
impl DomainPlugin for HospitalPlugin {
    fn name(&self) -> &str { "Hospital" }
    fn source_type(&self) -> &str { "Patient" }
    fn target_type(&self) -> &str { "Ward" }
    fn description(&self) -> &str { "Waiting HIGH/MED patients (>30 min) → appropriate ward" }
    fn is_distressed(&self, n: &OntologyNode, _g: &OntologyGraph) -> bool {
        n.node_type == "Patient"
            && n.get("status") == "WAITING"
            && matches!(n.get("acuity"), "HIGH" | "MED")
            && n.get_i64("wait_minutes") > 30
    }
    fn is_available(&self, n: &OntologyNode, _g: &OntologyGraph) -> bool {
        n.node_type == "Ward" && n.get_f64("occupancy") < 80.0
    }
    fn suggest_target(&self, src: &OntologyNode, candidates: &[OntologyNode], _g: &OntologyGraph) -> Option<(String, String)> {
        let preferred: &[&str] = match src.get("acuity") {
            "HIGH" => &["ICU", "SURGICAL"],
            _ => &["GENERAL", "SURGICAL"],
        };
        for pref in preferred {
            if let Some(n) = candidates.iter()
                .filter(|n| n.get("ward_type") == *pref)
                .min_by_key(|n| (n.get_f64("occupancy") * 10.0) as i64)
            {
                return Some((n.node_id.clone(),
                    format!("acuity={} → {} occ={}%", src.get("acuity"), pref, n.get("occupancy"))));
            }
        }
        candidates.iter()
            .min_by_key(|n| (n.get_f64("occupancy") * 10.0) as i64)
            .map(|n| (n.node_id.clone(), format!("fallback occ={}%", n.get("occupancy"))))
    }
}

// ── Plugin Registry ──────────────────────────────────────────

pub fn get_plugin(domain: &str) -> Box<dyn DomainPlugin> {
    match domain {
        "logistics" => Box::new(LogisticsPlugin),
        "incident" => Box::new(IncidentPlugin),
        "hospital" => Box::new(HospitalPlugin),
        _ => {
            eprintln!("Unknown domain '{}'. Use: logistics | incident | hospital | rail", domain);
            std::process::exit(1);
        }
    }
}

pub fn list_plugins() -> Vec<Box<dyn DomainPlugin>> {
    vec![
        Box::new(LogisticsPlugin),
        Box::new(IncidentPlugin),
        Box::new(HospitalPlugin),
    ]
}
