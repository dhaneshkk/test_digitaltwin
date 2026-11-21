//! Dashboard page — server-rendered with shard-based reactivity.
//! Each panel is a #[shard] that re-renders on the server.

use topcoat::prelude::*;
use crate::db::{self, Db};
use crate::engine;
use crate::models::*;

/// Helper: get db handle from request context.
/// In production you'd use Topcoat's context injection;
/// here we demonstrate the pattern.
async fn get_db(cx: &Cx) -> Db {
    // In a real Topcoat app, the Db would be injected via State.
    // This is a simplified pattern for demonstration.
    crate::DB.clone()
}

// ══════════════════════════════════════════════════════════════
// Dashboard Page
// ══════════════════════════════════════════════════════════════

#[page("/dashboard")]
pub async fn dashboard(cx: &Cx) -> Result {
    let db = get_db(cx).await;
    let ontologies = db::list_ontologies(&db).unwrap_or_default();

    view! {
        <!DOCTYPE html>
        <html lang="en" class="dark">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"Dashboard — Digital Twin"</title>
                <link rel="stylesheet" href=(topcoat::tailwind::stylesheet!())>
                topcoat::dev::script()
            </head>
            <body style="background: #0a0e17; color: #e2e8f0; font-family: system-ui;">
                // ── Top Bar ──
                <nav class="flex items-center gap-3 px-6 py-3 border-b"
                     style="border-color: #1e293b; background: #111827;">
                    <div style="width:8px;height:8px;border-radius:50%;background:#06b6d4;box-shadow:0 0 8px #06b6d4;"></div>
                    <a href="/" class="text-sm font-extrabold tracking-widest" style="color:#e2e8f0;">"DIGITAL TWIN"</a>
                    <span style="color:#475569;font-size:12px;">"/ Dashboard"</span>
                </nav>

                <div class="flex min-h-screen">
                    // ── Sidebar: Ontology List ──
                    <aside class="w-64 border-r p-3 flex-shrink-0"
                           style="border-color:#1e293b; background:#1a2234;">
                        <div class="text-xs font-bold tracking-wider mb-3 px-2"
                             style="color:#94a3b8;">"ONTOLOGIES"</div>
                        for onto in &ontologies {
                            <a href=(format!("/dashboard/onto/{}", onto["id"].as_str().unwrap_or("")))
                               class="block p-3 rounded-md mb-1 hover:bg-[#111827] transition-colors"
                               style="border:1px solid transparent;">
                                <div class="text-sm font-semibold"
                                     style="color:#e2e8f0;">
                                    (onto["name"].as_str().unwrap_or(""))
                                </div>
                                <div class="text-xs mt-1" style="color:#475569;">
                                    "v" (onto["version"].as_str().unwrap_or("")) " · "
                                    (onto["n_runs"].as_i64().unwrap_or(0).to_string()) " runs"
                                </div>
                            </a>
                        }
                        if ontologies.is_empty() {
                            <div class="text-center py-8 text-xs" style="color:#475569;">
                                "No ontologies yet"
                            </div>
                        }
                    </aside>

                    // ── Main Content ──
                    <main class="flex-1 p-6">
                        <div class="text-center py-20">
                            <div class="text-5xl opacity-15 mb-4">"🏭"</div>
                            <div class="text-sm" style="color:#475569;">
                                "Select an ontology from the sidebar, or create one via the API"
                            </div>
                            <pre class="mt-4 text-xs p-4 rounded-lg inline-block text-left"
                                 style="background:#111827; color:#94a3b8;">
"curl -X POST http://localhost:3000/api/ontologies \\
  -H 'Content-Type: application/json' \\
  -d @ontology.json"
                            </pre>
                        </div>
                    </main>
                </div>
            </body>
        </html>
    }
}

// ══════════════════════════════════════════════════════════════
// Ontology Detail Page
// ══════════════════════════════════════════════════════════════

#[page("/dashboard/onto/{id}")]
pub async fn onto_detail(cx: &Cx, id: String) -> Result {
    let db = get_db(cx).await;
    let onto_full = match db::get_ontology_full(&db, &id) {
        Ok(Some(v)) => v,
        _ => return view! { <h1>"Not found"</h1> },
    };
    let onto: BusinessOntology = match db::get_ontology(&db, &id) {
        Ok(Some(o)) => o,
        _ => return view! { <h1>"Not found"</h1> },
    };

    let validation = engine::validate_ontology(&onto);
    let ifd: Vec<_> = {
        let mut actions: Vec<_> = onto.actions.iter().collect();
        actions.sort_by(|a, b| b.1.ifd_score().cmp(&a.1.ifd_score()));
        actions
    };
    let token_map = onto.token_map();
    let runs = db::list_runs(&db, &id).unwrap_or_default();
    let audit = db::list_audit(&db, &id, 50).unwrap_or_default();
    let stats = db::user_stats(&db, &id).unwrap_or_default();
    let sam_history = db::list_sam(&db, &id).unwrap_or_default();

    view! {
        <!DOCTYPE html>
        <html lang="en" class="dark">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>(onto.name.clone()) " — Digital Twin"</title>
                <link rel="stylesheet" href=(topcoat::tailwind::stylesheet!())>
                topcoat::dev::script()
                <style>"
                    body { background: #0a0e17; color: #e2e8f0; font-family: system-ui; }
                    .card { background: #111827; border: 1px solid #1e293b; border-radius: 8px; overflow: hidden; }
                    .card-hdr { padding: 12px 16px; border-bottom: 1px solid #1e293b;
                                background: #1a2234; font-size: 13px; font-weight: 700;
                                text-transform: uppercase; letter-spacing: 0.5px; }
                    .badge { display: inline-block; padding: 2px 10px; border-radius: 4px;
                             font-size: 11px; font-weight: 700; }
                    table { width: 100%; border-collapse: collapse; }
                    th { text-align:left; padding:8px; color:#94a3b8; font-size:11px;
                         text-transform:uppercase; border-bottom:2px solid #1e293b; }
                    td { padding:10px 8px; border-bottom:1px solid #1e293b; font-size:13px; }
                    .stat { text-align:center; }
                    .stat-v { font-size:28px; font-weight:800; font-family:monospace; }
                    .stat-l { font-size:10px; color:#475569; text-transform:uppercase; letter-spacing:1px; }
                "</style>
            </head>
            <body>
                // ── Nav ──
                <nav class="flex items-center gap-3 px-6 py-3 border-b"
                     style="border-color:#1e293b; background:#111827;">
                    <div style="width:8px;height:8px;border-radius:50%;background:#06b6d4;box-shadow:0 0 8px #06b6d4;"></div>
                    <a href="/" class="text-sm font-extrabold tracking-widest" style="color:#e2e8f0;">"DIGITAL TWIN"</a>
                    <span style="color:#475569;">"/"</span>
                    <a href="/dashboard" style="color:#94a3b8;font-size:13px;">"Dashboard"</a>
                    <span style="color:#475569;">"/"</span>
                    <span class="font-semibold text-sm">(onto.name.clone())</span>
                    <span class="badge" style="background:#06b6d422;color:#06b6d4;border:1px solid #06b6d444;">
                        "v" (onto.version.clone())
                    </span>
                    <span class="badge" style="background:transparent;color:#475569;border:1px solid #1e293b;">
                        (onto.fingerprint())
                    </span>
                </nav>

                <div class="p-6 max-w-7xl mx-auto space-y-6">
                    // ── Stats Row ──
                    <div class="grid grid-cols-4 gap-4">
                        <div class="card p-6 stat">
                            <div class="stat-v" style="color:#06b6d4;">(onto.initial_entities.len().to_string())</div>
                            <div class="stat-l">"Entities"</div>
                        </div>
                        <div class="card p-6 stat">
                            <div class="stat-v" style="color:#06b6d4;">(onto.actions.len().to_string())</div>
                            <div class="stat-l">"Actions"</div>
                        </div>
                        <div class="card p-6 stat">
                            <div class="stat-v" style="color:#06b6d4;">(onto.users().len().to_string())</div>
                            <div class="stat-l">"Users"</div>
                        </div>
                        <div class="card p-6 stat">
                            <div class="stat-v" style="color:#06b6d4;">(onto.max_timesteps.to_string())</div>
                            <div class="stat-l">"Max Timesteps"</div>
                        </div>
                    </div>

                    // ── Validation ──
                    <div class="card">
                        <div class="card-hdr flex justify-between items-center">
                            <span>"Validation"</span>
                            if validation.is_valid {
                                <span class="badge" style="background:#22c55e22;color:#22c55e;">"✅ PASSED"</span>
                            } else {
                                <span class="badge" style="background:#ef444422;color:#ef4444;">
                                    "❌ " (validation.errors.len().to_string()) " ERROR(S)"
                                </span>
                            }
                        </div>
                        <div class="p-4">
                            for err in &validation.errors {
                                <div class="text-xs mb-1" style="color:#ef4444;">"• " (err.as_str())</div>
                            }
                            for warn in &validation.warnings {
                                <div class="text-xs mb-1" style="color:#f59e0b;">"⚠ " (warn.as_str())</div>
                            }
                            if validation.is_valid && validation.warnings.is_empty() {
                                <div class="text-xs" style="color:#22c55e;">"All checks passed."</div>
                            }
                        </div>
                    </div>

                    // ── IFD Priority Matrix ──
                    <div class="card">
                        <div class="card-hdr">"IFD Pilot Priority Matrix (Furr & Shipilov)"</div>
                        <div class="p-4">
                            <table>
                                <thead>
                                    <tr>
                                        <th>"Action"</th><th>"I"</th><th>"F"</th><th>"D"</th>
                                        <th>"Score"</th><th>"Priority"</th><th>"Customer Impact"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    for (name, adef) in &ifd {
                                        <tr>
                                            <td style="font-family:monospace;color:#06b6d4;font-weight:600;">
                                                (name.as_str())
                                            </td>
                                            <td>
                                                <span class="badge" style=(format!(
                                                    "background:{}22;color:{};",
                                                    if adef.intensity == IfdScore::High {"#ef4444"} else {"#94a3b8"},
                                                    if adef.intensity == IfdScore::High {"#ef4444"} else {"#94a3b8"}
                                                ))>(adef.intensity.label())</span>
                                            </td>
                                            <td>
                                                <span class="badge" style=(format!(
                                                    "background:{}22;color:{};",
                                                    if adef.frequency == IfdScore::High {"#ef4444"} else {"#94a3b8"},
                                                    if adef.frequency == IfdScore::High {"#ef4444"} else {"#94a3b8"}
                                                ))>(adef.frequency.label())</span>
                                            </td>
                                            <td>
                                                <span class="badge" style=(format!(
                                                    "background:{}22;color:{};",
                                                    if adef.density == IfdScore::High {"#ef4444"} else {"#94a3b8"},
                                                    if adef.density == IfdScore::High {"#ef4444"} else {"#94a3b8"}
                                                ))>(adef.density.label())</span>
                                            </td>
                                            <td style="font-weight:800;font-family:monospace;">
                                                (adef.ifd_score().to_string()) "/9"
                                            </td>
                                            <td>
                                                <span class="badge" style=(match adef.ifd_priority() {
                                                    "CRITICAL" => "background:#ef444422;color:#ef4444;",
                                                    "HIGH" => "background:#f59e0b22;color:#f59e0b;",
                                                    _ => "background:#22c55e22;color:#22c55e;",
                                                })>(adef.ifd_priority())</span>
                                            </td>
                                            <td class="text-xs" style="color:#94a3b8;">
                                                if adef.customer_impact.is_empty() {
                                                    <span style="color:#f59e0b;">"⚠ UNDEFINED"</span>
                                                } else {
                                                    (adef.customer_impact.as_str())
                                                }
                                            </td>
                                        </tr>
                                    }
                                </tbody>
                            </table>
                        </div>
                    </div>

                    // ── Capability Matrix ──
                    <div class="card">
                        <div class="card-hdr">"Capability Matrix (OCap Security)"</div>
                        <div class="p-4 grid grid-cols-3 gap-3">
                            for (user, tokens) in &token_map {
                                <div class="p-3 rounded-lg" style="background:#1a2234;border:1px solid #1e293b;">
                                    <div class="font-bold text-sm mb-2">"👤 " (user.as_str())</div>
                                    for tok in tokens {
                                        <div class="text-xs mb-1">
                                            <span style="color:#06b6d4;font-weight:600;">"🔑 " (tok.as_str())</span>
                                            <span style="color:#475569;"> " → ["
                                                for (aname, adef) in &onto.actions {
                                                    if &adef.required_capability == tok {
                                                        (aname.as_str()) " "
                                                    }
                                                }
                                            "]"</span>
                                        </div>
                                    }
                                </div>
                            }
                        </div>
                    </div>

                    // ── Entities ──
                    <div class="card">
                        <div class="card-hdr">"Entities (" (onto.initial_entities.len().to_string()) ")"</div>
                        <div class="p-4 grid grid-cols-3 gap-3">
                            for ent in &onto.initial_entities {
                                <div class="p-3 rounded-lg" style="background:#1a2234;border:1px solid #1e293b;">
                                    <div class="font-bold text-sm mb-2" style="color:#06b6d4;">
                                        (ent.entity_id.as_str())
                                    </div>
                                    for (p, v) in &ent.properties {
                                        <div class="text-xs" style="color:#94a3b8;">
                                            (p.as_str()) ": "
                                            <span style="color:#e2e8f0;">(v.as_str())</span>
                                        </div>
                                    }
                                </div>
                            }
                        </div>
                    </div>

                    // ── Recent Runs ──
                    <div class="card">
                        <div class="card-hdr">"Simulation Runs (" (runs.len().to_string()) ")"</div>
                        <div class="p-4">
                            if runs.is_empty() {
                                <div class="text-center text-xs py-4" style="color:#475569;">
                                    "No runs yet — POST to /api/ontologies/" (id.as_str()) "/simulate"
                                </div>
                            } else {
                                for run in &runs {
                                    <div class="flex items-center gap-3 py-2 border-b" style="border-color:#1e293b;">
                                        if run["has_model"].as_bool().unwrap_or(false) {
                                            <span class="badge" style="background:#22c55e22;color:#22c55e;">
                                                "✅ " (run["n_successes"].as_i64().unwrap_or(0).to_string()) "S"
                                            </span>
                                        } else {
                                            <span class="badge" style="background:#ef444422;color:#ef4444;">"NO MODEL"</span>
                                        }
                                        if run["n_errors"].as_i64().unwrap_or(0) > 0 {
                                            <span class="badge" style="background:#ef444422;color:#ef4444;">
                                                (run["n_errors"].as_i64().unwrap_or(0).to_string()) "E"
                                            </span>
                                        }
                                        <span class="text-xs" style="color:#94a3b8;">
                                            (run["run_label"].as_str().unwrap_or(""))
                                        </span>
                                        <span class="text-xs" style="color:#475569;">
                                            (run["created_at"].as_str().unwrap_or(""))
                                        </span>
                                    </div>
                                }
                            }
                        </div>
                    </div>

                    // ── Audit Trail ──
                    <div class="card">
                        <div class="card-hdr">"Governance Audit Trail (" (audit.len().to_string()) " entries)"</div>
                        <div class="p-4" style="max-height:300px;overflow:auto;">
                            if audit.is_empty() {
                                <div class="text-center text-xs py-4" style="color:#475569;">"No audit entries"</div>
                            } else {
                                for entry in &audit {
                                    <div class="flex items-center gap-2 py-1 border-b text-xs"
                                         style="border-color:#1e293b;">
                                        if entry["outcome"].as_str() == Some("success") {
                                            <span>"✅"</span>
                                        } else {
                                            <span>"❌"</span>
                                        }
                                        <span style="color:#475569;font-family:monospace;">
                                            "T=" (entry["timestep"].as_i64().unwrap_or(0).to_string())
                                        </span>
                                        <span>
                                            (entry["user_id"].as_str().unwrap_or("")) " → "
                                            (entry["action"].as_str().unwrap_or("")) "("
                                            (entry["target"].as_str().unwrap_or("")) ")"
                                        </span>
                                        <span class="badge" style=(if entry["outcome"].as_str() == Some("success") {
                                            "background:#22c55e22;color:#22c55e;"
                                        } else {
                                            "background:#ef444422;color:#ef4444;"
                                        })>(entry["outcome"].as_str().unwrap_or(""))</span>
                                    </div>
                                }
                            }
                        </div>
                    </div>

                    // ── User Stats ──
                    if !stats.is_empty() {
                        <div class="card">
                            <div class="card-hdr">"User Statistics"</div>
                            <div class="p-4 grid grid-cols-3 gap-3">
                                for s in &stats {
                                    <div class="text-center p-3 rounded-lg"
                                         style="background:#1a2234;border:1px solid #1e293b;">
                                        <div class="font-bold text-sm">(s.user_id.as_str())</div>
                                        <div class="stat-v" style="color:#06b6d4;">
                                            (format!("{:.0}%", s.success_rate))
                                        </div>
                                        <div class="stat-l">
                                            (s.successes.to_string()) "/" (s.total.to_string()) " success"
                                        </div>
                                    </div>
                                }
                            </div>
                        </div>
                    }

                    // ── SAM History ──
                    if !sam_history.is_empty() {
                        <div class="card">
                            <div class="card-hdr">"SAM Robustness History"</div>
                            <div class="p-4">
                                for h in &sam_history {
                                    <div class="flex items-center gap-4 py-2 border-b text-xs"
                                         style="border-color:#1e293b;">
                                        <span style="color:#475569;">
                                            (h["created_at"].as_str().unwrap_or(""))
                                        </span>
                                        <span style="font-weight:700;font-family:monospace;"
                                              class=(if h["mcc"].as_f64().unwrap_or(0.0) > 0.5 {
                                                  "text-green-400"
                                              } else { "text-yellow-400" })>
                                            "MCC=" (format!("{:.4}", h["mcc"].as_f64().unwrap_or(0.0)))
                                        </span>
                                        <span style="color:#94a3b8;">
                                            "κ=" (format!("{:.4}", h["cohens_kappa"].as_f64().unwrap_or(0.0)))
                                        </span>
                                        <span style="color:#475569;">
                                            (h["n_perturbations"].as_i64().unwrap_or(0).to_string()) " perturbations"
                                        </span>
                                    </div>
                                }
                            </div>
                        </div>
                    }
                </div>
            </body>
        </html>
    }
}
