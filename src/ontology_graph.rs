//! Petgraph-based ontology graph with rerouting + Lean4-verified invariant.
//! reroute_single_outgoing: after reroute, source has exactly 1 outgoing edge.

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct OntologyNode {
    pub node_id: String,
    pub node_type: String,
    pub properties: HashMap<String, String>,
}

impl OntologyNode {
    pub fn new(id: &str, nt: &str, props: &[(&str, &str)]) -> Self {
        Self {
            node_id: id.into(),
            node_type: nt.into(),
            properties: props.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }
    pub fn get(&self, k: &str) -> &str {
        self.properties.get(k).map(|s| s.as_str()).unwrap_or("")
    }
    pub fn get_f64(&self, k: &str) -> f64 {
        self.properties.get(k).and_then(|v| v.parse().ok()).unwrap_or(0.0)
    }
    pub fn get_i64(&self, k: &str) -> i64 {
        self.properties.get(k).and_then(|v| v.parse().ok()).unwrap_or(0)
    }
}

pub struct TxEntry {
    pub src: String,
    pub old_tgt: String,
    pub new_tgt: String,
}

pub struct OntologyGraph {
    g: DiGraph<OntologyNode, ()>,
    idx: HashMap<String, NodeIndex>,
    pub tx_log: Vec<TxEntry>,
}

impl OntologyGraph {
    pub fn new() -> Self {
        Self {
            g: DiGraph::new(),
            idx: HashMap::new(),
            tx_log: vec![],
        }
    }

    pub fn add(&mut self, node: OntologyNode, link_to: Option<&str>) {
        let id = node.node_id.clone();
        let ni = self.g.add_node(node);
        self.idx.insert(id, ni);
        if let Some(tgt) = link_to {
            if let Some(&ti) = self.idx.get(tgt) {
                self.g.add_edge(ni, ti, ());
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&OntologyNode> {
        self.idx.get(id).map(|&ni| &self.g[ni])
    }

    pub fn all(&self) -> impl Iterator<Item = &OntologyNode> {
        self.g.node_weights()
    }

    pub fn out_neighbors(&self, id: &str) -> Vec<String> {
        match self.idx.get(id) {
            Some(&ni) => self.g.edges_directed(ni, Direction::Outgoing)
                .map(|e| self.g[e.target()].node_id.clone())
                .collect(),
            None => vec![],
        }
    }

    /// Reroute src → tgt, enforcing Lean4-verified reroute_single_outgoing invariant.
    pub fn reroute(&mut self, src: &str, tgt: &str) -> Result<(), String> {
        let &si = self.idx.get(src).ok_or(format!("src '{}' not found", src))?;
        let &ti = self.idx.get(tgt).ok_or(format!("tgt '{}' not found", tgt))?;
        let old = self.out_neighbors(src).into_iter().next().unwrap_or_default();
        let eids: Vec<_> = self.g.edges_directed(si, Direction::Outgoing).map(|e| e.id()).collect();
        for eid in eids {
            self.g.remove_edge(eid);
        }
        self.g.add_edge(si, ti, ());
        let count = self.g.edges_directed(si, Direction::Outgoing).count();
        if count != 1 {
            return Err(format!("reroute_single_outgoing violated: {} edges", count));
        }
        self.tx_log.push(TxEntry { src: src.into(), old_tgt: old, new_tgt: tgt.into() });
        Ok(())
    }

    pub fn node_count(&self) -> usize { self.g.node_count() }
    pub fn edge_count(&self) -> usize { self.g.edge_count() }
}

/// Build demo graph for a given domain
pub fn build_graph(domain: &str) -> OntologyGraph {
    let mut g = OntologyGraph::new();
    match domain {
        "logistics" => {
            g.add(OntologyNode::new("WH-A", "Warehouse", &[("capacity_percent", "95"), ("has_cold_storage", "false")]), None);
            g.add(OntologyNode::new("WH-B", "Warehouse", &[("capacity_percent", "42"), ("has_cold_storage", "true")]), None);
            g.add(OntologyNode::new("WH-C", "Warehouse", &[("capacity_percent", "10"), ("has_cold_storage", "false")]), None);
            g.add(OntologyNode::new("T-100", "Shipment", &[("status", "DELAYED"), ("cargo_temp_c", "5.2")]), Some("WH-A"));
            g.add(OntologyNode::new("T-101", "Shipment", &[("status", "ON_TIME"), ("cargo_temp_c", "-18.5")]), Some("WH-B"));
            g.add(OntologyNode::new("T-103", "Shipment", &[("status", "DELAYED"), ("cargo_temp_c", "3.1")]), Some("WH-A"));
        }
        "incident" => {
            g.add(OntologyNode::new("SRV-1", "Service", &[("cpu_usage", "85"), ("incident_count", "4")]), None);
            g.add(OntologyNode::new("SRV-2", "Service", &[("cpu_usage", "40"), ("incident_count", "1")]), None);
            g.add(OntologyNode::new("SRV-3", "Service", &[("cpu_usage", "20"), ("incident_count", "0")]), None);
            g.add(OntologyNode::new("INC-001", "Incident", &[("status", "OPEN"), ("priority", "P1")]), Some("SRV-1"));
            g.add(OntologyNode::new("INC-002", "Incident", &[("status", "OPEN"), ("priority", "P1")]), Some("SRV-1"));
            g.add(OntologyNode::new("INC-003", "Incident", &[("status", "CLOSED"), ("priority", "P2")]), Some("SRV-2"));
        }
        "hospital" => {
            g.add(OntologyNode::new("WARD-ICU", "Ward", &[("ward_type", "ICU"), ("occupancy", "90")]), None);
            g.add(OntologyNode::new("WARD-SURG", "Ward", &[("ward_type", "SURGICAL"), ("occupancy", "55")]), None);
            g.add(OntologyNode::new("WARD-GEN", "Ward", &[("ward_type", "GENERAL"), ("occupancy", "30")]), None);
            g.add(OntologyNode::new("PAT-001", "Patient", &[("status", "WAITING"), ("acuity", "HIGH"), ("wait_minutes", "45")]), Some("WARD-ICU"));
            g.add(OntologyNode::new("PAT-002", "Patient", &[("status", "WAITING"), ("acuity", "MED"), ("wait_minutes", "60")]), Some("WARD-ICU"));
            g.add(OntologyNode::new("PAT-003", "Patient", &[("status", "STABLE"), ("acuity", "LOW"), ("wait_minutes", "10")]), Some("WARD-GEN"));
        }
        _ => {}
    }
    g
}
