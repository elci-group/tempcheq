use crate::run::{run_audit, RunOptions};
use anyhow::Result;
use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::Duration;

pub fn watch(root: &Path, opts: &RunOptions) -> Result<()> {
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    println!("Watching {} for changes... (Ctrl+C to stop)\n", root.display());
    run_audit(root, opts)?;

    loop {
        match rx.recv_timeout(Duration::from_secs(3600)) {
            Ok(Ok(_event)) => {
                // Debounce: drain any immediately-following events.
                while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
                print!("\x1B[2J\x1B[1;1H");
                println!("Change detected, re-auditing {}...\n", root.display());
                if let Err(e) = run_audit(root, opts) {
                    eprintln!("audit error: {e}");
                }
            }
            Ok(Err(e)) => eprintln!("watch error: {e}"),
            Err(_) => continue,
        }
    }
}
