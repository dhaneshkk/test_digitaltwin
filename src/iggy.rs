//! Apache Iggy message bus integration [IGG-01].
//! Publishes audit, engine events, and telemetry to Iggy streams.
//! Falls back to local buffering when Iggy is unreachable.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use std::time::Duration;

pub struct IggyBus {
    base: String,
    token: Option<String>,
    pub connected: bool,
    client: reqwest::blocking::Client,
    pub buf_audit: Vec<Value>,
    pub buf_events: Vec<Value>,
    pub buf_telem: Vec<Value>,
    pub published: usize,
    pub fallback: usize,
}

impl IggyBus {
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            base: format!("http://{}:{}", host, port),
            token: None,
            connected: false,
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
            buf_audit: vec![],
            buf_events: vec![],
            buf_telem: vec![],
            published: 0,
            fallback: 0,
        }
    }

    pub fn connect(&mut self) {
        let url = format!("{}/users/login", self.base);
        let body = json!({"username": "iggy", "password": "iggy"});
        if let Ok(r) = self.client.post(&url).json(&body).send() {
            if r.status().is_success() {
                if let Ok(j) = r.json::<Value>() {
                    if let Some(tok) = j["access_token"].as_str() {
                        self.token = Some(tok.to_string());
                        self.connected = true;
                        self.ensure_topology();
                        println!("  [Iggy] ✓ {}", self.base);
                        return;
                    }
                }
            }
        }
        println!("  [Iggy] ⚠ offline — buffering locally");
    }

    fn ensure_topology(&self) {
        let tok = match &self.token {
            Some(t) => t.as_str(),
            None => return,
        };
        let _ = self
            .client
            .post(format!("{}/streams", self.base))
            .bearer_auth(tok)
            .json(&json!({"stream_id": 1, "name": "digital-twin"}))
            .send();
        for (id, name) in &[
            (1u32, "governance-audit"),
            (2, "engine-events"),
            (3, "telemetry"),
        ] {
            let _ = self
                .client
                .post(format!("{}/streams/1/topics", self.base))
                .bearer_auth(tok)
                .json(&json!({"topic_id": id, "name": name, "partitions_count": 1, "message_expiry": 0}))
                .send();
        }
    }

    fn send_to(&mut self, topic_id: u32, rec: Value) {
        let mut sent = false;
        if self.connected {
            let payload = B64.encode(rec.to_string().as_bytes());
            let body = json!({
                "partitioning": {"kind": "partition_id", "value": "1"},
                "messages": [{"id": 0, "payload": payload}]
            });
            let url = format!("{}/streams/1/topics/{}/messages", self.base, topic_id);
            let tok = self.token.clone().unwrap_or_default();
            match self.client.post(&url).bearer_auth(&tok).json(&body).send() {
                Ok(r) if r.status().is_success() => {
                    self.published += 1;
                    sent = true;
                }
                _ => {
                    self.connected = false;
                }
            }
        }
        if !sent {
            self.fallback += 1;
            match topic_id {
                1 => self.buf_audit.push(rec),
                2 => self.buf_events.push(rec),
                _ => self.buf_telem.push(rec),
            }
        }
    }

    pub fn emit_audit(&mut self, r: Value) {
        self.send_to(1, r);
    }
    pub fn emit_event(&mut self, r: Value) {
        self.send_to(2, r);
    }
    pub fn emit_telem(&mut self, r: Value) {
        self.send_to(3, r);
    }

    pub fn status_line(&self) -> String {
        format!(
            "connected={} published={} fallback={} buf=[{},{},{}]",
            self.connected,
            self.published,
            self.fallback,
            self.buf_audit.len(),
            self.buf_events.len(),
            self.buf_telem.len()
        )
    }

    pub fn stats(&self) -> Value {
        json!({
            "connected": self.connected,
            "iggy_published": self.published,
            "fallback_buffered": self.fallback,
        })
    }
}
