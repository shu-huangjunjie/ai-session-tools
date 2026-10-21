// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;

fn main() {
    let crash_log = context_vault_lib::paths::AppPaths::discover(None)
        .map(|p| p.logs.join("crash.log"))
        .unwrap_or_else(|_| std::env::temp_dir().join("context-vault-crash.log"));

    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_default();
        let entry = format!(
            "[{}] PANIC: {} at {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            message,
            location
        );
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&crash_log)
        {
            let _ = file.write_all(entry.as_bytes());
        }
        eprint!("{}", entry);
    }));

    context_vault_lib::run();
}
