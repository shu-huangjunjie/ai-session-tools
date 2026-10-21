//! Manual smoke test against the histories installed on this computer.
//! Client folders are only read; recovered data goes to a temporary vault.
//! Run with: cargo test --test local_history_smoke -- --ignored --nocapture

use context_vault_lib::paths::AppPaths;
use context_vault_lib::persistence::{Database, Repository};
use context_vault_lib::services::{
    AntigravityRecoveryService, ClaudeRecoveryService, CodexRecoveryService,
};
use std::time::Instant;

#[test]
#[ignore = "Reads the Codex, Claude and Antigravity histories installed on this computer"]
fn scan_and_recover_local_histories() {
    let root = std::env::temp_dir().join(format!("cv-smoke-{}", uuid::Uuid::new_v4()));
    let paths = AppPaths::discover(Some(&root)).unwrap();
    let db = Database::new(&paths.database, &paths.backups).unwrap();

    let started = Instant::now();
    let codex = CodexRecoveryService::new(&db, None);
    let report = codex.scan();
    println!(
        "codex: {} candidates, {} indexes, {} missing, warnings {:?} ({:?})",
        report.candidates.len(),
        report.index_count,
        report.missing_rollouts,
        report.warnings,
        started.elapsed()
    );
    let started = Instant::now();
    let parsed: usize = report
        .candidates
        .iter()
        .filter(|c| !c.history_only)
        .map(|c| CodexRecoveryService::parse_rollout(std::path::Path::new(&c.rollout_path)).len())
        .sum();
    println!("codex parse only: {} messages ({:?})", parsed, started.elapsed());
    let started = Instant::now();
    let result = codex.recover(&report.candidates);
    println!(
        "codex recover: {} conversations, {} messages, {} skipped ({:?})",
        result.conversation_count,
        result.message_count,
        result.skipped_count,
        started.elapsed()
    );

    let started = Instant::now();
    let claude = ClaudeRecoveryService::new(&db, Some(&paths.claude_home()));
    let report = claude.scan();
    println!(
        "claude: {} candidates, warnings {:?} ({:?})",
        report.candidates.len(),
        report.warnings,
        started.elapsed()
    );
    for c in &report.candidates {
        println!("  claude candidate: {} | {:?}", c.title, c.project_path);
    }
    let result = claude.recover(&report.candidates);
    println!(
        "claude recover: {} conversations, {} messages",
        result.conversation_count, result.message_count
    );

    let started = Instant::now();
    let antigravity = AntigravityRecoveryService::new(&db, Some(&paths.antigravity_home()));
    let report = antigravity.scan();
    println!(
        "antigravity: {} candidates, warnings {:?} ({:?})",
        report.candidates.len(),
        report.warnings,
        started.elapsed()
    );
    let result = antigravity.recover(&report.candidates);
    println!(
        "antigravity recover: {} conversations, {} messages",
        result.conversation_count, result.message_count
    );

    let repo = Repository::new(&db);
    for query in ["数据库", "error", "优化"] {
        let started = Instant::now();
        let hits = repo
            .search_conversations_with_archived(query, 100, true)
            .unwrap();
        println!("search {:?}: {} hits ({:?})", query, hits.len(), started.elapsed());
    }

    drop(db);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "Copies the vault database of this computer and migrates the copy"]
fn migrate_copy_of_local_vault() {
    use context_vault_lib::persistence::backup_sqlite;
    let source = AppPaths::discover(None).unwrap().database;
    if !source.is_file() {
        println!("no local vault at {}", source.display());
        return;
    }
    let root = std::env::temp_dir().join(format!("cv-migrate-copy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let copy = root.join("conversations.db");
    backup_sqlite(&source, &copy).unwrap();

    let started = Instant::now();
    let db = Database::new(&copy, &root.join("backups")).unwrap();
    println!("migration: {:?}", started.elapsed());
    let repo = Repository::new(&db);
    let active = repo.list_conversation_summaries(false, None).unwrap();
    let archived = repo.list_conversation_summaries(true, None).unwrap();
    println!("conversations: {} active, {} archived", active.len(), archived.len());
    for query in ["数据库", "优化", "error", "rust 优化"] {
        let started = Instant::now();
        let hits = repo
            .search_conversations_with_archived(query, 200, true)
            .unwrap();
        println!("search {:?}: {} hits ({:?})", query, hits.len(), started.elapsed());
    }
    db.lock_conn()
        .execute("INSERT INTO message_fts(message_fts) VALUES('integrity-check')", [])
        .unwrap();

    // Recovering again must only add content that is new since the last import.
    let duplicates = |db: &Database| -> i64 {
        db.lock_conn()
            .query_row(
                "SELECT COALESCE(SUM(n - 1), 0) FROM (
                    SELECT COUNT(*) AS n FROM messages
                    GROUP BY conversation_id, role, content HAVING n > 1)",
                [],
                |r| r.get(0),
            )
            .unwrap()
    };
    let duplicates_before = duplicates(&db);
    let paths = AppPaths::discover(None).unwrap();
    let codex = CodexRecoveryService::new(&db, None);
    let report = codex.scan();
    let existing: Vec<_> = report.candidates.iter().filter(|c| c.exists_locally).cloned().collect();
    let result = codex.recover(&existing);
    println!(
        "codex re-recover of {} imported: {} new conversations, {} new messages",
        existing.len(),
        result.new_conversation_count,
        result.message_count
    );
    println!(
        "repeated messages before {} / after {}",
        duplicates_before,
        duplicates(&db)
    );
    let claude = ClaudeRecoveryService::new(&db, Some(&paths.claude_home()));
    let report = claude.scan();
    let existing: Vec<_> = report.candidates.iter().filter(|c| c.exists_locally).cloned().collect();
    let result = claude.recover(&existing);
    println!(
        "claude re-recover of {} imported: {} new conversations, {} new messages",
        existing.len(),
        result.new_conversation_count,
        result.message_count
    );
    let antigravity = AntigravityRecoveryService::new(&db, Some(&paths.antigravity_home()));
    let report = antigravity.scan();
    let existing: Vec<_> = report.candidates.iter().filter(|c| c.exists_locally).cloned().collect();
    let result = antigravity.recover(&existing);
    println!(
        "antigravity re-recover of {} imported: {} new conversations, {} new messages",
        existing.len(),
        result.new_conversation_count,
        result.message_count
    );

    drop(db);
    println!(
        "size after migration: {} bytes",
        std::fs::metadata(&copy).unwrap().len()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[ignore = "Reads the Codex sidebar state of this computer and a copy of a vault"]
fn codex_visibility_of_local_vault() {
    use context_vault_lib::persistence::backup_sqlite;
    use context_vault_lib::services::CodexScope;
    // CV_SMOKE_VAULT selects another vault, such as a portable copy's data/conversations.db.
    let source = std::env::var_os("CV_SMOKE_VAULT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| AppPaths::discover(None).unwrap().database);
    if !source.is_file() {
        println!("no vault at {}", source.display());
        return;
    }
    let root = std::env::temp_dir().join(format!("cv-visibility-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let copy = root.join("conversations.db");
    backup_sqlite(&source, &copy).unwrap();
    let db = Database::new(&copy, &root.join("backups")).unwrap();

    let codex = CodexRecoveryService::new(&db, None);
    let report = codex.scan();
    let count = |scope| report.candidates.iter().filter(|c| c.codex_scope == Some(scope)).count();
    println!(
        "scan: {} candidates; project {}, chat {}, hidden {}, unknown {}",
        report.candidates.len(),
        count(CodexScope::Project),
        count(CodexScope::Chat),
        count(CodexScope::Hidden),
        report.candidates.iter().filter(|c| c.codex_scope.is_none()).count()
    );

    let started = Instant::now();
    let visibility = codex.visibility().unwrap();
    println!("visibility ({:?}): available {}", started.elapsed(), visibility.available);
    let titles: std::collections::HashMap<_, _> = Repository::new(&db)
        .list_conversation_summaries(false, None)
        .unwrap()
        .into_iter()
        .chain(Repository::new(&db).list_conversation_summaries(true, None).unwrap())
        .map(|c| (c.id, c.title))
        .collect();
    for project in &visibility.projects {
        println!("project {} {:?}", project.name, project.root_paths);
    }
    for (id, placement) in &visibility.conversations {
        let project = placement
            .project_id
            .as_ref()
            .and_then(|p| visibility.projects.iter().find(|x| &x.project_id == p))
            .map(|p| p.name.as_str());
        println!("  {:?} {:?}: {}", placement.scope, project, titles.get(id).map(String::as_str).unwrap_or("?"));
    }
    println!("listed {} of {} vault conversations", visibility.conversations.len(), titles.len());
    drop(db);
    let _ = std::fs::remove_dir_all(&root);
}
