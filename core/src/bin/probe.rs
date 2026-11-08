//! Headless pipeline check, no window. Usage:
//!   cargo run --bin probe -- index <folder>
//!   cargo run --bin probe -- search "what I remember"
//!   cargo run --bin probe -- status

use recall_core::{boot, db, ingest, search};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("recall_core=info").init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model_dir = PathBuf::from("models");
    let app = boot(model_dir)?;

    match args.first().map(|s| s.as_str()) {
        Some("index") => {
            let folder = args.get(1).expect("folder");
            db::add_source(&app.db, folder)?;
            ingest::sweep(&app.ingest.tx, &app.db, &app.index, folder);
            // give the worker time; in the app this runs forever
            std::thread::sleep(std::time::Duration::from_secs(20));
            app.ingest.tx.send(ingest::Job::Save)?;
            std::thread::sleep(std::time::Duration::from_secs(1));
            println!("chunks: {}", db::chunk_count(&app.db)?);
        }
        Some("search") => {
            let q = args[1..].join(" ");
            std::thread::sleep(std::time::Duration::from_secs(3)); // let the model load
            let r = search::run(&app.db, &app.index, &app.ingest.embedder, &search::Query { q, ..Default::default() })?;
            println!("{} cards, {} hits, {} ms", r.cards.len(), r.total_hits, r.took_ms);
            for c in r.cards.iter().take(10) {
                println!("\n[{}] {}  ({} · {} · {})  {:.2}", c.kind, c.title, c.location, &c.modified_at[..10], c.size_label, c.score);
                for h in &c.hits {
                    let t: String = h.text.chars().take(110).collect();
                    println!("   {:>6}  {}", h.locator, t.replace('\n', " "));
                }
            }
        }
        _ => {
            println!("chunks: {}  vectors: {}  embedder: {}", db::chunk_count(&app.db)?, app.index.read().len(), app.ingest.embedder.lock().is_some());
            for s in db::list_sources(&app.db)? {
                println!("source {} ({} items)", s.path, s.count);
            }
        }
    }
    Ok(())
}
