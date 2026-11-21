//! Root layout — `<html>` shell with Tailwind, shared nav.

use topcoat::prelude::*;

#[page("/")]
pub async fn home() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en" class="dark">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"Digital Twin Engine v3"</title>
                <link rel="stylesheet" href=(topcoat::tailwind::stylesheet!())>
                topcoat::dev::script()
                <style>"
                    :root {
                        --bg: #0a0e17; --surface: #111827; --surface-alt: #1a2234;
                        --border: #1e293b; --text: #e2e8f0; --text-dim: #94a3b8;
                        --accent: #06b6d4; --success: #22c55e; --error: #ef4444;
                        --warn: #f59e0b;
                    }
                    body { background: var(--bg); color: var(--text); font-family: system-ui; }
                    .card { background: var(--surface); border: 1px solid var(--border);
                            border-radius: 8px; overflow: hidden; }
                    .card-header { padding: 12px 16px; border-bottom: 1px solid var(--border);
                                   background: var(--surface-alt); font-size: 13px;
                                   font-weight: 700; text-transform: uppercase;
                                   letter-spacing: 0.5px; }
                    .btn { padding: 8px 18px; border-radius: 6px; font-size: 13px;
                           font-weight: 600; cursor: pointer; transition: all 0.15s;
                           border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
                           background: color-mix(in srgb, var(--accent) 15%, transparent);
                           color: var(--accent); }
                    .btn:hover { background: color-mix(in srgb, var(--accent) 25%, transparent); }
                    .badge { display: inline-block; padding: 2px 10px; border-radius: 4px;
                             font-size: 11px; font-weight: 700; letter-spacing: 0.5px; }
                    .stat-val { font-size: 28px; font-weight: 800; font-family: monospace; }
                    .stat-label { font-size: 10px; color: var(--text-dim); text-transform: uppercase;
                                  letter-spacing: 1px; }
                    table { width: 100%; border-collapse: collapse; }
                    th { text-align: left; padding: 8px; color: var(--text-dim); font-size: 11px;
                         text-transform: uppercase; border-bottom: 2px solid var(--border); }
                    td { padding: 10px 8px; border-bottom: 1px solid var(--border); }
                    select, input { background: var(--surface-alt); border: 1px solid var(--border);
                                    border-radius: 6px; padding: 5px 8px; color: var(--text);
                                    font-size: 12px; }
                    .timeline-step { margin-bottom: 14px; padding-left: 16px;
                                     border-left: 3px solid color-mix(in srgb, var(--accent) 25%, transparent); }
                    .pulse { width: 8px; height: 8px; border-radius: 50%;
                             background: var(--accent); box-shadow: 0 0 8px var(--accent); }
                "</style>
            </head>
            <body class="min-h-screen">
                <nav class="flex items-center gap-3 px-6 py-3 border-b"
                     style="border-color: var(--border); background: var(--surface)">
                    <div class="pulse"></div>
                    <span class="text-sm font-extrabold tracking-widest">"DIGITAL TWIN ENGINE"</span>
                    <span class="text-xs" style="color: var(--text-dim)">"v3 · Rust + Topcoat + SQLite"</span>
                    <div class="flex-1"></div>
                    <a href="/dashboard" class="btn text-xs">"Dashboard →"</a>
                </nav>
                <main class="p-6 max-w-7xl mx-auto">
                    <div class="text-center py-20">
                        <div class="text-6xl opacity-15 mb-4">"🏭"</div>
                        <h1 class="text-2xl font-bold mb-2">"Universal Digital Twin Engine"</h1>
                        <p style="color: var(--text-dim)" class="mb-6">
                            "MIT friction engineering + HBR/IFD experimentation discipline"
                        </p>
                        <div class="flex justify-center gap-4">
                            <a href="/dashboard" class="btn">"Open Dashboard"</a>
                            <a href="/api/health" class="btn" style="
                                background: transparent; color: var(--text-dim);
                                border-color: var(--border);">"API Health →"</a>
                        </div>
                    </div>
                    <div class="grid grid-cols-3 gap-4 max-w-3xl mx-auto mt-8">
                        <div class="card text-center p-6">
                            <div class="stat-val" style="color: var(--accent)">"Pure Rust"</div>
                            <div class="stat-label">"No C dependencies"</div>
                        </div>
                        <div class="card text-center p-6">
                            <div class="stat-val" style="color: var(--success)">"SQLite"</div>
                            <div class="stat-label">"Embedded DB"</div>
                        </div>
                        <div class="card text-center p-6">
                            <div class="stat-val" style="color: var(--warn)">"SHA-256"</div>
                            <div class="stat-label">"Audit chain"</div>
                        </div>
                    </div>
                </main>
            </body>
        </html>
    }
}
