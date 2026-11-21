//! Operator AI Q&A via OpenAI-compatible /chat/completions [IGG-07].
//! Uses Gemini as backend through its OpenAI-compatible endpoint.

use crate::engine::GovernanceRegistry;
use crate::iggy::IggyBus;
use crate::planner::TelemetryRecord;
use serde_json::json;

pub fn operator_qa(
    question: &str,
    registry: &GovernanceRegistry,
    telem: &[TelemetryRecord],
    bus: &IggyBus,
    api_key: &str,
) -> String {
    if api_key.is_empty() {
        return "[Set GEMINI_API_KEY or --api-key]".into();
    }

    let distressed = telem.iter().filter(|r| r.label == 1).count();
    let last5: Vec<String> = registry.entries.iter().rev().take(5).rev()
        .map(|e| format!("t={} {}  {} → {}", e.timestep, e.user, e.action, e.outcome))
        .collect();

    let context = format!(
        "=== CONTEXT ===\nIggy: {}\nAudit entries: {}  Chain: {}\n\
         Audit (last 5):\n  {}\nTelemetry: {} steps, {} distressed\n===============",
        bus.status_line(),
        registry.entries.len(),
        registry.verify_chain(),
        last5.join("\n  "),
        telem.len(),
        distressed,
    );

    let body = json!({
        "model": "gemini-2.0-flash",
        "messages": [
            {
                "role": "system",
                "content": "Operator AI for a Digital Twin Platform (Apache Iggy backend). Answer strictly from context. Be concise."
            },
            {
                "role": "user",
                "content": format!("{context}\n\nQuestion: {question}")
            }
        ],
        "max_tokens": 256,
        "temperature": 0.1
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();

    match client
        .post("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
    {
        Ok(r) => r.json::<serde_json::Value>().ok()
            .and_then(|j| j["choices"][0]["message"]["content"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "[empty response]".into()),
        Err(e) => format!("[HTTP error: {}]", e),
    }
}
