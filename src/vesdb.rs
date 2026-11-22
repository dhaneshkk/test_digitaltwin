//! Vesdb client — tamper-evident event store integration.
//!
//! Replaces the hand-rolled SHA-256 chain in SQLite with Vesdb's:
//!   - Merkle tree (inclusion proofs for any single event)
//!   - Signed receipts (writer key signs each batch)
//!   - External witnesses (cosign checkpoints, refuse forks)
//!   - Lean4-proved no_fork, ack_durable, receipt_sound
//!
//! Vesdb exposes an EventSourcingDB-compatible HTTP API.
//! This client calls it for audit writes and verification.
//! When Vesdb is unreachable, falls back to local SQLite audit.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

// ══════════════════════════════════════════════════════════════
// Event types for the audit stream
// ══════════════════════════════════════════════════════════════

/// A Digital Twin audit event stored in Vesdb.
/// Each event is one action attempt with its outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub run_id: String,
    pub ontology_id: String,
    pub timestep: usize,
    pub user_id: String,
    pub action: String,
    pub target: String,
    pub outcome: String,    // "success" | "error_capability" | "error_state" | "EXECUTED" | "COMMITTED"
    pub detail: String,
    pub domain: String,     // "rail" | "logistics" | "incident" | "hospital" | "simulation"
    #[serde(default)]
    pub idempotency_key: String,
}

impl AuditEvent {
    /// CloudEvents-style subject for Vesdb stream filtering
    pub fn subject(&self) -> String {
        format!("dtw.{}.{}.{}", self.domain, self.action, self.target)
    }

    /// Compute salted commitment (matches Vesdb's data/<commitment> naming)
    pub fn commitment(&self, salt: &[u8; 32]) -> String {
        let payload = serde_json::to_vec(self).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(salt);
        hasher.update(&payload);
        hex::encode(hasher.finalize())
    }
}

/// Receipt returned by Vesdb after a successful write batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteReceipt {
    pub position: u64,
    pub merkle_root: String,
    pub receipt_signature: String,
    pub event_ids: Vec<u64>,
    pub timestamp: String,
}

/// Verification result from Vesdb
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    pub valid: bool,
    pub entries_checked: usize,
    pub merkle_root: String,
    pub last_position: u64,
    pub last_receipt: Option<String>,
    pub witness_cosignatures: usize,
    pub errors: Vec<String>,
}

/// Inclusion proof for a single event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InclusionProof {
    pub event_id: u64,
    pub leaf_hash: String,
    pub merkle_path: Vec<String>,
    pub root: String,
    pub receipt_signature: String,
    pub valid: bool,
}

// ══════════════════════════════════════════════════════════════
// Vesdb Client
// ══════════════════════════════════════════════════════════════

pub struct VesdbClient {
    base_url: String,
    stream: String,
    client: reqwest::blocking::Client,
    pub connected: bool,
    pub events_written: usize,
    pub receipts: Vec<WriteReceipt>,
    // Fallback buffer when Vesdb is unreachable
    pub fallback_buffer: Vec<AuditEvent>,
}

impl VesdbClient {
    pub fn new(base_url: &str, stream: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            stream: stream.to_string(),
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            connected: false,
            events_written: 0,
            receipts: vec![],
            fallback_buffer: vec![],
        }
    }

    /// Connect to Vesdb and ensure the audit stream exists.
    pub fn connect(&mut self) -> bool {
        // Check health
        let health_url = format!("{}/health/ready", self.base_url);
        match self.client.get(&health_url).send() {
            Ok(r) if r.status().is_success() => {
                self.connected = true;
                self.ensure_stream();
                println!("  [Vesdb] ✓ {} stream={}", self.base_url, self.stream);
                true
            }
            Ok(r) => {
                println!("  [Vesdb] ⚠ health returned {}", r.status());
                false
            }
            Err(e) => {
                println!("  [Vesdb] ⚠ offline ({}) — audit falls back to SQLite", e);
                false
            }
        }
    }

    /// Ensure the audit stream exists in Vesdb
    fn ensure_stream(&self) {
        let url = format!("{}/api/streams", self.base_url);
        let _ = self.client.post(&url)
            .json(&json!({
                "name": &self.stream,
                "subject_filter": "dtw.**",
            }))
            .send();
    }

    /// Write a batch of audit events to Vesdb.
    /// Returns a receipt on success, or falls back to buffer.
    pub fn write_batch(
        &mut self,
        events: &[AuditEvent],
        idempotency_key: Option<&str>,
    ) -> Result<WriteReceipt, String> {
        if events.is_empty() {
            return Err("empty batch".into());
        }

        if !self.connected {
            self.fallback_buffer.extend(events.iter().cloned());
            return Err("Vesdb offline — buffered locally".into());
        }

        let url = format!("{}/api/streams/{}/events", self.base_url, self.stream);

        // Build CloudEvents-style payload
        let ce_events: Vec<Value> = events.iter().map(|e| {
            json!({
                "type": "dtw.audit",
                "subject": e.subject(),
                "data": serde_json::to_value(e).unwrap_or_default(),
            })
        }).collect();

        let mut req = self.client.post(&url).json(&json!({
            "events": ce_events,
        }));

        if let Some(key) = idempotency_key {
            req = req.header("Idempotency-Key", key);
        }

        match req.send() {
            Ok(r) if r.status().is_success() => {
                match r.json::<Value>() {
                    Ok(body) => {
                        let receipt = WriteReceipt {
                            position: body["position"].as_u64().unwrap_or(0),
                            merkle_root: body["merkleRoot"].as_str().unwrap_or("").to_string(),
                            receipt_signature: body["receipt"].as_str().unwrap_or("").to_string(),
                            event_ids: body["eventIds"].as_array()
                                .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
                                .unwrap_or_default(),
                            timestamp: body["timestamp"].as_str().unwrap_or("").to_string(),
                        };
                        self.events_written += events.len();
                        self.receipts.push(receipt.clone());
                        Ok(receipt)
                    }
                    Err(e) => {
                        self.fallback_buffer.extend(events.iter().cloned());
                        Err(format!("Vesdb response parse error: {}", e))
                    }
                }
            }
            Ok(r) if r.status().as_u16() == 412 => {
                // Precondition failed — position conflict (another writer)
                Err("Position conflict: another writer claimed this position".into())
            }
            Ok(r) => {
                self.connected = false;
                self.fallback_buffer.extend(events.iter().cloned());
                Err(format!("Vesdb write failed: HTTP {}", r.status()))
            }
            Err(e) => {
                self.connected = false;
                self.fallback_buffer.extend(events.iter().cloned());
                Err(format!("Vesdb connection error: {}", e))
            }
        }
    }

    /// Write a single audit event (convenience wrapper).
    pub fn write_event(&mut self, event: AuditEvent) -> Result<WriteReceipt, String> {
        let key = if event.idempotency_key.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            event.idempotency_key.clone()
        };
        self.write_batch(&[event], Some(&key))
    }

    /// Verify the entire audit stream — Merkle tree, receipts, witnesses.
    pub fn verify(&self) -> Result<VerifyResult, String> {
        if !self.connected {
            return Err("Vesdb offline".into());
        }

        let url = format!("{}/api/streams/{}/verify", self.base_url, self.stream);
        match self.client.get(&url).send() {
            Ok(r) if r.status().is_success() => {
                r.json::<VerifyResult>().map_err(|e| e.to_string())
            }
            Ok(r) => Err(format!("Verify failed: HTTP {}", r.status())),
            Err(e) => Err(format!("Connection error: {}", e)),
        }
    }

    /// Get inclusion proof for a specific event.
    pub fn inclusion_proof(&self, event_id: u64) -> Result<InclusionProof, String> {
        if !self.connected {
            return Err("Vesdb offline".into());
        }

        let url = format!("{}/api/streams/{}/events/{}/proof",
            self.base_url, self.stream, event_id);
        match self.client.get(&url).send() {
            Ok(r) if r.status().is_success() => {
                r.json::<InclusionProof>().map_err(|e| e.to_string())
            }
            Ok(r) => Err(format!("Proof failed: HTTP {}", r.status())),
            Err(e) => Err(format!("Connection error: {}", e)),
        }
    }

    /// Read events from the stream (for audit trail display).
    pub fn read_events(
        &self,
        from_position: u64,
        limit: usize,
    ) -> Result<Vec<AuditEvent>, String> {
        if !self.connected {
            return Err("Vesdb offline".into());
        }

        let url = format!(
            "{}/api/streams/{}/events?from={}&limit={}",
            self.base_url, self.stream, from_position, limit
        );
        match self.client.get(&url).send() {
            Ok(r) if r.status().is_success() => {
                match r.json::<Value>() {
                    Ok(body) => {
                        let events = body["events"].as_array()
                            .map(|arr| {
                                arr.iter().filter_map(|v| {
                                    serde_json::from_value::<AuditEvent>(
                                        v["data"].clone()
                                    ).ok()
                                }).collect()
                            })
                            .unwrap_or_default();
                        Ok(events)
                    }
                    Err(e) => Err(format!("Parse error: {}", e)),
                }
            }
            Ok(r) => Err(format!("Read failed: HTTP {}", r.status())),
            Err(e) => Err(format!("Connection error: {}", e)),
        }
    }

    /// Flush fallback buffer to Vesdb (call after reconnection).
    pub fn flush_buffer(&mut self) -> usize {
        if !self.connected || self.fallback_buffer.is_empty() {
            return 0;
        }
        let buffered = std::mem::take(&mut self.fallback_buffer);
        let count = buffered.len();
        // Write in batches of 50
        for chunk in buffered.chunks(50) {
            let key = Uuid::new_v4().to_string();
            if self.write_batch(chunk, Some(&key)).is_err() {
                // Put remaining back
                self.fallback_buffer.extend(chunk.iter().cloned());
                break;
            }
        }
        count - self.fallback_buffer.len()
    }

    pub fn status_line(&self) -> String {
        format!(
            "connected={} written={} receipts={} buffered={}",
            self.connected, self.events_written,
            self.receipts.len(), self.fallback_buffer.len()
        )
    }

    pub fn stats(&self) -> Value {
        json!({
            "connected": self.connected,
            "events_written": self.events_written,
            "receipts": self.receipts.len(),
            "fallback_buffered": self.fallback_buffer.len(),
            "last_merkle_root": self.receipts.last()
                .map(|r| r.merkle_root.clone())
                .unwrap_or_default(),
        })
    }
}
