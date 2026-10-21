use context_vault_lib::importers::{parse_file, ImportService};
use context_vault_lib::persistence::{Database, ImportedMessagePayload, Repository};
use context_vault_lib::services::{AntigravityPublisher, ClaudePublisher};
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> (PathBuf, Database) {
    let root = std::env::temp_dir().join(format!("cv-{}-{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let db = Database::new(&root.join("vault.db"), &root.join("backups")).unwrap();
    (root, db)
}

fn message(id: &str, parent: Option<&str>, content: &str) -> ImportedMessagePayload {
    ImportedMessagePayload {
        role: "user".into(),
        content: content.into(),
        external_id: id.into(),
        parent_external_id: parent.map(str::to_string),
        created_at: None,
    }
}

#[test]
fn folder_pins_normalize_paths_survive_restarts_and_clear_with_their_source() {
    use context_vault_lib::persistence::ResetScope;
    let (root, db) = fixture("folder-pins");
    let repo = Repository::new(&db);
    repo.set_folder_pinned("codex", r"\\?\C:\Work\", true).unwrap();
    let original_stamp = repo.list_folder_pins().unwrap()[0].pinned_at.clone();
    repo.set_folder_pinned("codex", "c:/work", true).unwrap();
    let pins = repo.list_folder_pins().unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].path, r"c:\work");
    assert_eq!(pins[0].pinned_at, original_stamp);
    repo.set_folder_pinned("claude", "C:/Work/", true).unwrap();
    assert_eq!(repo.list_folder_pins().unwrap().len(), 2);
    assert!(repo.set_folder_pinned("unknown", "C:/Work", true).is_err());
    let backup = db.backup(None).unwrap();
    drop(repo);
    drop(db);

    let db = Database::new(&root.join("vault.db"), &root.join("backups")).unwrap();
    let repo = Repository::new(&db);
    assert_eq!(repo.list_folder_pins().unwrap().len(), 2);
    repo.set_folder_pinned("codex", r"C:\WORK\", false).unwrap();
    assert_eq!(repo.list_folder_pins().unwrap()[0].source, "claude");
    repo.set_folder_pinned("codex", "C:/Work", true).unwrap();
    let preview = repo.reset_conversations(ResetScope::Codex, true, true).unwrap();
    assert_eq!(preview.folder_pin_count, 1);
    assert_eq!(repo.list_folder_pins().unwrap().len(), 2);
    repo.reset_conversations(ResetScope::Codex, true, false).unwrap();
    assert_eq!(repo.list_folder_pins().unwrap().len(), 1);
    assert_eq!(repo.list_folder_pins().unwrap()[0].source, "claude");
    repo.reset_conversations(ResetScope::All, true, false).unwrap();
    assert!(repo.list_folder_pins().unwrap().is_empty());
    assert_eq!(Connection::open(backup).unwrap().query_row(
        "SELECT COUNT(*) FROM folder_pins", [], |r| r.get::<_, i64>(0),
    ).unwrap(), 2);
}

#[test]
fn version_8_database_adds_folder_pins_and_preserves_conversation_pins() {
    let (root, db) = fixture("folder-pin-migration");
    let repo = Repository::new(&db);
    let (id, _, _, _) = repo.sync_import_conversation(
        "Keep conversation pin", "test", "existing", &[message("m", None, "Keep contents")],
        None, Some("C:/Work"), "general",
    ).unwrap();
    repo.set_pinned(&id, true).unwrap();
    let pinned_at = repo.get_conversation(&id).unwrap().unwrap().pinned_at;
    drop(repo);
    db.lock_conn().execute_batch("DROP TABLE folder_pins; PRAGMA user_version = 8;").unwrap();
    drop(db);
    let db = Database::new(&root.join("vault.db"), &root.join("backups")).unwrap();
    let repo = Repository::new(&db);
    assert_eq!(db.schema_version().unwrap(), 9);
    assert_eq!(repo.get_conversation(&id).unwrap().unwrap().pinned_at, pinned_at);
    assert!(repo.list_folder_pins().unwrap().is_empty());
    repo.set_folder_pinned("codex", "C:/Work", true).unwrap();
    assert_eq!(repo.list_folder_pins().unwrap().len(), 1);
    let backup = fs::read_dir(root.join("backups")).unwrap().next().unwrap().unwrap().path();
    let original = Connection::open(backup).unwrap();
    assert_eq!(original.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 8);
    assert_eq!(original.query_row("SELECT content FROM messages", [], |r| r.get::<_, String>(0)).unwrap(), "Keep contents");
}

#[test]
fn pins_survive_rescans_restarts_and_backups_without_changing_recency() {
    let (root, db) = fixture("persistent-pins");
    let repo = Repository::new(&db);
    let payload = [message("m", None, "pin-search-keyword")];
    let (id, _, _, _) = repo.sync_import_conversation(
        "Pin fixture", "test", "pinned-session", &payload, None, None, "general",
    ).unwrap();
    let updated_at = repo.get_conversation(&id).unwrap().unwrap().updated_at;
    assert!(repo.set_pinned(&id, true).unwrap());
    let pinned_at = repo.get_conversation(&id).unwrap().unwrap().pinned_at.unwrap();
    assert!(repo.set_pinned(&id, true).unwrap());
    let pinned = repo.get_conversation(&id).unwrap().unwrap();
    assert_eq!(pinned.pinned_at.as_deref(), Some(pinned_at.as_str()));
    assert_eq!(pinned.updated_at, updated_at);
    repo.sync_import_conversation(
        "Newer unpinned", "test", "unpinned-session", &[message("m", None, "keyword")],
        None, None, "general",
    ).unwrap();
    assert_eq!(repo.list_conversation_summaries(false, None).unwrap()[0].pinned_at, Some(pinned_at.clone()));
    assert_eq!(repo.search_conversations("keyword", 10).unwrap()[0].pinned_at, Some(pinned_at.clone()));
    assert_eq!(repo.search_conversations("keyword", 1).unwrap()[0].id, id);
    repo.sync_import_conversation(
        "Pin fixture", "test", "pinned-session", &payload, None, None, "general",
    ).unwrap();
    repo.set_archived(&id, true).unwrap();
    assert_eq!(repo.list_conversation_summaries(true, None).unwrap()[0].pinned_at, Some(pinned_at.clone()));
    assert!(!repo.set_pinned("does-not-exist", true).unwrap());
    drop(repo);
    drop(db);

    let db = Database::new(&root.join("vault.db"), &root.join("backups")).unwrap();
    let repo = Repository::new(&db);
    assert_eq!(repo.get_conversation(&id).unwrap().unwrap().pinned_at, Some(pinned_at.clone()));
    let backup = db.backup(None).unwrap();
    assert!(repo.set_pinned(&id, false).unwrap());
    assert!(repo.get_conversation(&id).unwrap().unwrap().pinned_at.is_none());
    let saved_pin: String = Connection::open(&backup).unwrap()
        .query_row("SELECT pinned_at FROM conversations WHERE id = ?1", [&id], |r| r.get(0)).unwrap();
    assert_eq!(saved_pin, pinned_at);
    repo.delete_conversation(&id).unwrap();
    assert!(repo.list_conversation_summaries(true, None).unwrap().is_empty());
}

#[test]
fn version_7_database_adds_pins_and_backs_up_existing_data() {
    let (root, db) = fixture("pin-migration");
    Repository::new(&db).sync_import_conversation(
        "Existing conversation", "test", "existing", &[message("m", None, "keep this message")],
        None, None, "general",
    ).unwrap();
    db.lock_conn().execute_batch(
        "ALTER TABLE conversations DROP COLUMN pinned_at; PRAGMA user_version = 7;",
    ).unwrap();
    drop(db);
    let db = Database::new(&root.join("vault.db"), &root.join("backups")).unwrap();
    assert_eq!(db.schema_version().unwrap(), context_vault_lib::persistence::database::SCHEMA_VERSION);
    let conversations = Repository::new(&db).list_conversation_summaries(false, None).unwrap();
    assert_eq!(conversations.len(), 1);
    assert_eq!(conversations[0].message_count, 1);
    assert!(conversations[0].pinned_at.is_none());
    let backup = fs::read_dir(root.join("backups")).unwrap().next().unwrap().unwrap().path();
    let original = Connection::open(backup).unwrap();
    assert_eq!(original.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 7);
    assert_eq!(original.query_row("SELECT content FROM messages", [], |r| r.get::<_, String>(0)).unwrap(), "keep this message");
}

#[test]
fn chatgpt_parents_resolve_after_children_and_repair_existing_imports() {
    let (root, db) = fixture("parent-order");
    let input = root.join("conversations.json");
    fs::write(&input, r#"[{"id":"session","title":"Example","current_node":"a-child",
        "mapping":{
          "a-child":{"parent":"z-parent","message":{"author":{"role":"assistant"},"content":{"parts":["Answer"]}}},
          "z-parent":{"parent":null,"message":{"author":{"role":"user"},"content":{"parts":["Question"]}}}
        }}]"#).unwrap();
    let parsed = parse_file(&input, "chatgpt_export").unwrap().remove(0);
    let payloads: Vec<_> = parsed
        .messages
        .iter()
        .map(|m| ImportedMessagePayload {
            role: m.role.clone(),
            content: m.content.clone(),
            external_id: m.external_id.clone(),
            parent_external_id: m.parent_external_id.clone(),
            created_at: m.created_at.clone(),
        })
        .collect();
    let repo = Repository::new(&db);
    let sync = || {
        repo.sync_import_conversation(
            &parsed.title,
            "chatgpt_export",
            &parsed.external_id,
            &payloads,
            parsed.active_external_id.as_deref(),
            None,
            "general",
        )
        .unwrap()
    };
    let (id, count, _, _) = sync();
    assert_eq!(count, 2);
    let path = repo.load_active_path(&id).unwrap();
    assert_eq!(
        path.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
        ["Question", "Answer"]
    );
    db.lock_conn()
        .execute("UPDATE messages SET parent_message_id = NULL", [])
        .unwrap();
    let (_, inserted, created, _) = sync();
    assert_eq!(inserted, 0);
    assert!(!created);
    assert_eq!(repo.load_active_path(&id).unwrap().len(), 2);
}

#[test]
fn imports_with_same_filename_are_distinct_and_duplicates_are_idempotent() {
    for (extension, first, second) in [
        (
            "md",
            "# First\n\n## User\n\nFirst content",
            "# Second\n\n## User\n\nSecond content",
        ),
        (
            "jsonl",
            r#"{"role":"user","content":"First content"}"#,
            r#"{"role":"user","content":"Second content"}"#,
        ),
    ] {
        let (root, db) = fixture("import-identities");
        fs::create_dir(root.join("first")).unwrap();
        fs::create_dir(root.join("second")).unwrap();
        let a = root.join("first").join(format!("notes.{}", extension));
        let b = root.join("second").join(format!("notes.{}", extension));
        fs::write(&a, first).unwrap();
        fs::write(&b, second).unwrap();
        let service = ImportService::new(&db);
        assert_eq!(service.import_file(&a).unwrap().new_conversation_count, 1);
        assert_eq!(service.import_file(&b).unwrap().new_conversation_count, 1);
        let repeated = service.import_file(&a).unwrap();
        assert_eq!(repeated.new_conversation_count, 0);
        assert_eq!(repeated.message_count, 0);
        let repo = Repository::new(&db);
        let conversations = repo.list_conversations(false, None).unwrap();
        assert_eq!(conversations.len(), 2);
        let contents: Vec<_> = conversations
            .iter()
            .flat_map(|c| repo.list_messages(&c.id).unwrap())
            .map(|m| m.content)
            .collect();
        assert!(contents.contains(&"First content".to_string()));
        assert!(contents.contains(&"Second content".to_string()));
    }
}

#[test]
fn native_import_ids_stay_stable_when_messages_change() {
    let (root, db) = fixture("native-id");
    let path = root.join("session.json");
    let service = ImportService::new(&db);
    fs::write(&path, r#"{"conversation_id":"native-session","messages":[{"id":"message-1","role":"user","content":"initial"}]}"#).unwrap();
    service.import_file(&path).unwrap();
    fs::write(&path, r#"{"conversation_id":"native-session","messages":[{"id":"message-1","role":"user","content":"updated"}]}"#).unwrap();
    let result = service.import_file(&path).unwrap();
    assert_eq!(result.new_conversation_count, 0);
    let repo = Repository::new(&db);
    let conversations = repo.list_conversations(false, None).unwrap();
    assert_eq!(conversations.len(), 1);
    assert_eq!(
        repo.list_messages(&conversations[0].id).unwrap()[0].content,
        "updated"
    );
}

#[test]
fn failed_import_rolls_back_and_cyclic_paths_return_errors() {
    let (_, db) = fixture("transaction");
    let repo = Repository::new(&db);
    let mut invalid = message("bad", Some("first"), "invalid");
    invalid.role = "invalid-role".into();
    assert!(repo
        .sync_import_conversation(
            "Invalid",
            "test",
            "broken",
            &[message("first", None, "first"), invalid],
            Some("bad"),
            None,
            "general"
        )
        .is_err());
    assert!(repo.list_conversations(false, None).unwrap().is_empty());
    let (id, _, _, _) = repo
        .sync_import_conversation(
            "Cycle",
            "test",
            "cycle",
            &[message("a", Some("b"), "a"), message("b", Some("a"), "b")],
            Some("b"),
            None,
            "general",
        )
        .unwrap();
    assert!(repo.load_active_path(&id).is_err());
}

#[test]
fn archived_conversations_are_included_only_when_requested() {
    let (_, db) = fixture("archived");
    let repo = Repository::new(&db);
    for source in ["active", "archived"] {
        let (id, _, _, _) = repo
            .sync_import_conversation(
                source,
                "test",
                source,
                &[message("message", None, "shared keyword")],
                None,
                None,
                "general",
            )
            .unwrap();
        if source == "archived" {
            repo.set_archived(&id, true).unwrap();
        }
    }
    assert_eq!(
        repo.list_conversation_summaries(false, None).unwrap().len(),
        1
    );
    assert_eq!(
        repo.list_conversation_summaries(true, None).unwrap().len(),
        1
    );
    assert_eq!(repo.search_conversations("keyword", 100).unwrap().len(), 1);
    assert_eq!(
        repo.search_conversations_with_archived("keyword", 100, true)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        repo.search_conversations_with_archived("", 100, true)
            .unwrap()
            .len(),
        2
    );
}

fn claude_fixture(name: &str) -> (PathBuf, Database, String) {
    let (root, db) = fixture(name);
    let repo = Repository::new(&db);
    let (id, _, _, _) = repo
        .sync_import_conversation(
            "Updated title",
            "claude_code",
            "native-session",
            &[message("old-message", None, "Older recovered message")],
            None,
            Some("C:\\demo"),
            "claude",
        )
        .unwrap();
    let project = root.join("claude/projects/example");
    fs::create_dir_all(project.join("native-session")).unwrap();
    fs::write(project.join("native-session.jsonl"), "{\"uuid\":\"newer-message\",\"toolMetadata\":{\"keep\":true},\"content\":\"newer client data\"}\r\n").unwrap();
    fs::write(
        project.join("native-session/custom-title.json"),
        r#"{"customTitle":"Old title","pinned":true}"#,
    )
    .unwrap();
    fs::write(root.join("claude/history.jsonl"), "{\"sessionId\":\"native-session\",\"display\":\"Old title\",\"timestamp\":123,\"extra\":true}\nopaque history line\n").unwrap();
    (root, db, id)
}

#[test]
fn claude_keeps_newer_native_transcripts_and_backs_up_all_originals() {
    let (root, db, id) = claude_fixture("claude-preservation");
    let home = root.join("claude");
    let transcript = home.join("projects/example/native-session.jsonl");
    let original = fs::read(&transcript).unwrap();
    let original_history = fs::read(home.join("history.jsonl")).unwrap();
    let publisher = ClaudePublisher::new(&db, Some(&home), &root.join("backups"));
    let result = publisher.publish(&[id], false).unwrap();
    assert_eq!(result.updated_count, 1);
    assert_eq!(fs::read(&transcript).unwrap(), original);
    let backup = PathBuf::from(result.backup_path.unwrap());
    assert_eq!(
        fs::read(backup.join("projects/example/native-session.jsonl")).unwrap(),
        original
    );
    assert_eq!(
        fs::read(backup.join("history.jsonl")).unwrap(),
        original_history
    );
    assert!(backup
        .join("projects/example/native-session/custom-title.json")
        .is_file());
    let title: serde_json::Value = serde_json::from_slice(
        &fs::read(home.join("projects/example/native-session/custom-title.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(title["customTitle"], "Updated title");
    assert_eq!(title["pinned"], true);
    let history = fs::read_to_string(home.join("history.jsonl")).unwrap();
    assert!(history.contains("opaque history line"));
    let entry: serde_json::Value = serde_json::from_str(history.lines().next().unwrap()).unwrap();
    assert_eq!(entry["timestamp"], 123);
    assert_eq!(entry["extra"], true);
}

#[test]
fn claude_aborts_without_changing_files_when_backups_fail() {
    let (root, db, id) = claude_fixture("claude-backup-failure");
    let home = root.join("claude");
    let paths = [
        home.join("history.jsonl"),
        home.join("projects/example/native-session.jsonl"),
        home.join("projects/example/native-session/custom-title.json"),
    ];
    let originals: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
    let invalid_backup_root = root.join("not-a-directory");
    fs::write(&invalid_backup_root, "blocked").unwrap();
    let publisher = ClaudePublisher::new(&db, Some(&home), &invalid_backup_root);
    assert!(publisher.publish(&[id], false).is_err());
    for (path, original) in paths.iter().zip(originals) {
        assert_eq!(fs::read(path).unwrap(), original);
    }
}

#[test]
fn antigravity_aborts_before_database_updates_when_backups_fail() {
    let (root, db) = fixture("ag-backup-failure");
    let home = root.join("antigravity");
    fs::create_dir(&home).unwrap();
    let target = rusqlite::Connection::open(home.join("conversation_summaries.db")).unwrap();
    target
        .execute("CREATE TABLE sentinel(value TEXT)", [])
        .unwrap();
    target
        .execute("INSERT INTO sentinel VALUES('preserved')", [])
        .unwrap();
    let invalid_backup_root = root.join("not-a-directory");
    fs::write(&invalid_backup_root, "blocked").unwrap();
    let publisher = AntigravityPublisher::new(&db, Some(&home), &invalid_backup_root);
    assert!(publisher.publish(&["missing".into()]).is_err());
    let count: i64 = target
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'conversation_summaries'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    let value: String = target
        .query_row("SELECT value FROM sentinel", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, "preserved");
}

// ---------------------------------------------------------------------------
// Codex

fn codex_home(root: &std::path::Path) -> PathBuf {
    let home = root.join("codex");
    fs::create_dir_all(home.join("sessions/2026/10/01")).unwrap();
    fs::create_dir_all(home.join("sqlite")).unwrap();
    let state = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
    state
        .execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY, rollout_path TEXT, created_at INTEGER, updated_at INTEGER,
                source TEXT, model_provider TEXT, cwd TEXT, title TEXT, has_user_event INTEGER,
                archived INTEGER, first_user_message TEXT, model TEXT, thread_source TEXT,
                created_at_ms INTEGER, updated_at_ms INTEGER, recency_at INTEGER, recency_at_ms INTEGER,
                git_branch TEXT, name TEXT);
             CREATE TABLE thread_spawn_edges (parent_thread_id TEXT, child_thread_id TEXT);",
        )
        .unwrap();
    for (id, source, thread_source, provider) in [
        ("11111111-1111-1111-1111-111111111111", "vscode", "user", "openai"),
        ("22222222-2222-2222-2222-222222222222", "vscode", "user", "openai"),
        (
            "33333333-3333-3333-3333-333333333333",
            "{\"subagent\":{}}",
            "subagent",
            "context_restoration_hidden",
        ),
    ] {
        let rollout = home.join(format!(
            "sessions/2026/10/01/rollout-2026-10-01T10-00-00-{}.jsonl",
            id
        ));
        let meta = serde_json::json!({
            "timestamp": "2026-10-01T10:00:00Z",
            "type": "session_meta",
            "payload": {"id": id, "cwd": "C:\\demo", "thread_source": thread_source,
                        "source": serde_json::from_str::<serde_json::Value>(source)
                            .unwrap_or(serde_json::json!(source))}
        });
        fs::write(
            &rollout,
            format!(
                "{meta}\n\
                 {{\"timestamp\":\"2026-10-01T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"message\":\"question {id}\"}}}}\n\
                 {{\"timestamp\":\"2026-10-01T10:00:01Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"answer {id}\"}}}}\n"
            ),
        )
        .unwrap();
        state
            .execute(
                "INSERT INTO threads VALUES (?1, ?2, 1781576266, 1781576266, ?3, ?4, 'C:\\demo', ?5, 1, 0,
                    'question', 'gpt-5', ?6, 1781576266000, 1781576266000, NULL, NULL, 'main', NULL)",
                rusqlite::params![
                    id,
                    rollout.to_string_lossy(),
                    source,
                    provider,
                    format!("Thread {}", &id[..1]),
                    thread_source
                ],
            )
            .unwrap();
    }
    let catalog = rusqlite::Connection::open(home.join("sqlite/codex-dev.db")).unwrap();
    catalog
        .execute_batch(
            "CREATE TABLE local_thread_catalog (
                host_id TEXT, thread_id TEXT, display_title TEXT, source_created_at REAL,
                source_updated_at REAL, cwd TEXT, source_kind TEXT, model_provider TEXT, git_branch TEXT,
                observation_sequence INTEGER, missing_candidate INTEGER, thread_source TEXT,
                source_recency_at REAL, pending_observed_title INTEGER, PRIMARY KEY (host_id, thread_id));
             CREATE TABLE local_thread_catalog_sync_state (host_id TEXT PRIMARY KEY, observation_sequence INTEGER);
             CREATE TABLE local_thread_catalog_hosts (host_id TEXT PRIMARY KEY, host_kind TEXT);
             INSERT INTO local_thread_catalog VALUES ('local', '22222222-2222-2222-2222-222222222222', 'Thread 2',
                1781576266.0, 1781576266.0, 'C:\\demo', 'vscode', 'openai', 'main', 1, 0, 'user', 1781576266.0, 0);",
        )
        .unwrap();
    home
}

#[test]
fn codex_scan_reads_integer_timestamps_from_state_index() {
    use context_vault_lib::services::CodexRecoveryService;
    let (root, db) = fixture("codex-scan");
    let home = codex_home(&root);
    let report = CodexRecoveryService::new(&db, Some(&home)).scan();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // The subagent thread is excluded; both main threads come from the state index.
    assert_eq!(report.candidates.len(), 2);
    for c in &report.candidates {
        assert_eq!(c.updated_at, "2026-06-16T02:17:46+00:00");
        assert!(
            c.source_indexes.iter().any(|s| s == "当前 Codex"),
            "{:?}",
            c.source_indexes
        );
    }
}

#[test]
fn codex_publish_uses_thread_ids_and_rejects_unknown_selections() {
    use context_vault_lib::services::{CodexClientPublisher, CodexRecoveryService};
    let (root, db) = fixture("codex-publish");
    let home = codex_home(&root);
    let recovery = CodexRecoveryService::new(&db, Some(&home));
    let report = recovery.scan();
    recovery.recover(&report.candidates);

    let repo = Repository::new(&db);
    let vault_ids: Vec<String> = repo
        .list_conversations(false, None)
        .unwrap()
        .into_iter()
        .map(|c| c.id)
        .collect();
    let mapping = repo
        .source_ids_for_conversations(CodexRecoveryService::SOURCE_TYPE, &vault_ids)
        .unwrap();
    assert_eq!(mapping.len(), 2);

    let publisher = CodexClientPublisher::new(Some(&home), &root.join("backups"));
    // Vault IDs themselves are not Codex thread IDs: nothing may be hidden.
    assert!(publisher.publish(&vault_ids, &[]).is_err());
    let state = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
    let hidden: i64 = state
        .query_row(
            "SELECT COUNT(*) FROM threads
             WHERE model_provider = 'context_restoration_hidden' AND thread_source <> 'subagent'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hidden, 0);

    let first = "11111111-1111-1111-1111-111111111111".to_string();
    let preview = publisher.preview(&[first.clone()]).unwrap();
    assert_eq!(preview.selected_thread_count, 1);
    assert_eq!(preview.hidden_thread_count, 1);
    let result = publisher.publish(&[first], &[]).unwrap();
    assert_eq!(result.catalog_thread_count, 1);
    assert_eq!(result.hidden_thread_count, 1);
    let providers: Vec<(String, String)> = state
        .prepare("SELECT id, model_provider FROM threads ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(providers[0].1, "openai");
    assert_eq!(providers[1].1, "context_restoration_hidden");
    // A subagent hidden by an older version is restored, not hidden again.
    assert_eq!(providers[2].1, "openai");
}

#[test]
fn codex_threads_without_rollouts_are_recovered_from_the_history_database() {
    use context_vault_lib::services::CodexRecoveryService;
    let (root, db) = fixture("codex-history");
    let home = codex_home(&root);
    let first = "11111111-1111-1111-1111-111111111111";
    for entry in fs::read_dir(home.join("sessions/2026/10/01")).unwrap() {
        let path = entry.unwrap().path();
        if path.to_string_lossy().contains(first) {
            fs::remove_file(path).unwrap();
        }
    }
    let history = rusqlite::Connection::open(home.join("thread_history_1.sqlite")).unwrap();
    history
        .execute_batch(
            "CREATE TABLE thread_items (thread_id TEXT, turn_id TEXT, item_id TEXT,
                rollout_ordinal INTEGER, created_at_ms INTEGER, item_json TEXT, item_type TEXT);",
        )
        .unwrap();
    for (ordinal, kind, json) in [
        (1, "userMessage", r##"{"type":"userMessage","id":"u1","clientId":"c1","content":[{"type":"text","text":"# Files mentioned by the user:\n## a.png: C:/a.png\n## My request:\n恢复这个会话"}]}"##),
        (2, "reasoning", r#"{"type":"reasoning","id":"r1","summary":[],"content":[]}"#),
        (3, "agentMessage", r#"{"type":"agentMessage","id":"m1","text":"已恢复","phase":"final_answer"}"#),
    ] {
        history
            .execute(
                "INSERT INTO thread_items VALUES (?1, 't', ?2, ?3, 1781576266000, ?4, ?5)",
                rusqlite::params![first, format!("i{}", ordinal), ordinal, json, kind],
            )
            .unwrap();
    }

    let service = CodexRecoveryService::new(&db, Some(&home));
    let report = service.scan();
    assert_eq!(report.missing_rollouts, 0);
    let candidate = report
        .candidates
        .iter()
        .find(|c| c.thread_id == first)
        .unwrap();
    assert!(candidate.history_only);
    let result = service.recover(&report.candidates);
    assert_eq!(result.conversation_count, 2);
    let repo = Repository::new(&db);
    let recovered = repo
        .search_conversations("恢复这个会话", 10)
        .unwrap()
        .remove(0);
    let path = repo.load_active_path(&recovered.id).unwrap();
    assert_eq!(
        path.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
        ["恢复这个会话", "已恢复"]
    );
    // A second recovery is incremental and creates no duplicates.
    let again = service.recover(&service.scan().candidates);
    assert_eq!(again.message_count, 0);
}

#[test]
fn codex_sidebar_scope_follows_project_assignments_not_folders() {
    use context_vault_lib::services::{CodexRecoveryService, CodexScope};
    let (root, db) = fixture("codex-scope");
    let home = codex_home(&root);
    let first = "11111111-1111-1111-1111-111111111111";
    let second = "22222222-2222-2222-2222-222222222222";
    let write_state = |chats: &[&str]| {
        let state = serde_json::json!({
            "local-projects": {"p1": {"id": "p1", "name": "Demo", "rootPaths": ["C:\\demo"]}},
            "project-order": ["p1"],
            "thread-project-assignments": {
                "11111111-1111-1111-1111-111111111111": {"projectKind": "local", "projectId": "p1"}
            },
            "projectless-thread-ids": chats,
        });
        fs::write(home.join(".codex-global-state.json"), state.to_string()).unwrap();
    };
    write_state(&[]);

    let service = CodexRecoveryService::new(&db, Some(&home));
    let report = service.scan();
    assert_eq!(report.sidebar_projects.as_ref().unwrap()[0].name, "Demo");
    let candidate = |id: &str| report.candidates.iter().find(|c| c.thread_id == id).unwrap().clone();
    assert_eq!(candidate(first).codex_scope, Some(CodexScope::Project));
    assert_eq!(candidate(first).codex_project_id.as_deref(), Some("p1"));
    // The second thread is in the project's folder, but Codex lists only assigned threads.
    assert_eq!(candidate(second).codex_scope, Some(CodexScope::Hidden));
    assert_eq!(candidate(second).project_path.as_deref(), Some("C:\\demo"));

    service.recover(&report.candidates);
    let threads = Repository::new(&db)
        .source_ids_by_conversation(CodexRecoveryService::SOURCE_TYPE)
        .unwrap();
    assert_eq!(threads.len(), 2);
    let conversation = |thread: &str| {
        threads.iter().find(|(_, t)| t.as_str() == thread).unwrap().0.clone()
    };
    let visibility = service.visibility().unwrap();
    assert!(visibility.available);
    assert_eq!(visibility.conversations.len(), 1);
    assert_eq!(
        visibility.conversations[&conversation(first)].project_id.as_deref(),
        Some("p1")
    );

    // Chats outside any project are listed; archived threads are not.
    write_state(&[second]);
    rusqlite::Connection::open(home.join("state_5.sqlite"))
        .unwrap()
        .execute("UPDATE threads SET archived = 1 WHERE id = ?1", [first])
        .unwrap();
    let visibility = service.visibility().unwrap();
    assert_eq!(visibility.conversations.len(), 1);
    assert_eq!(visibility.conversations[&conversation(second)].scope, CodexScope::Chat);

    // Without project state the tab keeps grouping by folder.
    fs::remove_file(home.join(".codex-global-state.json")).unwrap();
    assert!(!service.visibility().unwrap().available);
    let report = service.scan();
    assert!(report.sidebar_projects.is_none());
    assert!(report.candidates.iter().all(|c| c.codex_scope.is_none()));
}

// ---------------------------------------------------------------------------
// Search

#[test]
fn search_matches_chinese_substrings_and_follows_deletes() {
    let (_, db) = fixture("search-cjk");
    let repo = Repository::new(&db);
    let (id, _, _, _) = repo
        .sync_import_conversation(
            "数据库调优",
            "test",
            "cjk",
            &[
                message("q", None, "请帮我优化数据库查询"),
                message("a", Some("q"), "Fix the DataBase error_code 100%"),
            ],
            None,
            None,
            "general",
        )
        .unwrap();
    for query in ["数据库", "优化", "查询", "base", "调优", "error_code", "100%", "优化 base"] {
        assert_eq!(
            repo.search_conversations(query, 10).unwrap().len(),
            1,
            "query {:?}",
            query
        );
    }
    for query in ["缓存", "errorXcode", "优化 缓存"] {
        assert!(
            repo.search_conversations(query, 10).unwrap().is_empty(),
            "query {:?}",
            query
        );
    }
    repo.delete_conversation(&id).unwrap();
    assert!(repo.search_conversations("数据库", 10).unwrap().is_empty());
    db.lock_conn()
        .execute(
            "INSERT INTO message_fts(message_fts) VALUES('integrity-check')",
            [],
        )
        .unwrap();
}

#[test]
fn version_6_databases_migrate_to_the_trigram_index_with_a_backup() {
    let root = std::env::temp_dir().join(format!("cv-migrate-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("vault.db");
    {
        let db = Database::new(&path, &root.join("backups")).unwrap();
        Repository::new(&db)
            .sync_import_conversation(
                "Old",
                "test",
                "old",
                &[message("m", None, "迁移之前的数据库内容")],
                None,
                None,
                "general",
            )
            .unwrap();
        // Recreate the version 6 index layout.
        db.lock_conn()
            .execute_batch(
                "DROP TRIGGER message_fts_ai; DROP TRIGGER message_fts_au; DROP TRIGGER message_fts_ad;
                 DROP TABLE message_fts;
                 CREATE VIRTUAL TABLE message_fts USING fts5(title, content, conversation_id UNINDEXED,
                     message_id UNINDEXED, tokenize = 'unicode61');
                 CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
                     INSERT INTO message_fts(title, content, conversation_id, message_id)
                     SELECT title, new.content, new.conversation_id, new.id
                     FROM conversations WHERE id = new.conversation_id;
                 END;
                 PRAGMA user_version = 6;",
            )
            .unwrap();
    }
    {
        // Version 6 stored the import time; the migration takes it from the messages.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "UPDATE messages SET created_at = '2025-05-05T05:05:05Z';
             UPDATE conversations SET created_at = '2026-10-01T00:00:00Z', updated_at = '2026-10-01T00:00:00Z';",
        )
        .unwrap();
    }
    let db = Database::new(&path, &root.join("backups")).unwrap();
    assert_eq!(db.schema_version().unwrap(), context_vault_lib::persistence::database::SCHEMA_VERSION);
    assert_eq!(fs::read_dir(root.join("backups")).unwrap().count(), 1);
    let repo = Repository::new(&db);
    let migrated = repo.list_conversations(false, None).unwrap().remove(0);
    assert_eq!(migrated.updated_at.to_rfc3339(), "2025-05-05T05:05:05+00:00");
    assert_eq!(repo.search_conversations("数据库", 10).unwrap().len(), 1);
    repo.sync_import_conversation(
        "New",
        "test",
        "new",
        &[message("m", None, "迁移之后的数据库内容")],
        None,
        None,
        "general",
    )
    .unwrap();
    assert_eq!(repo.search_conversations("数据库内容", 10).unwrap().len(), 2);
}

#[test]
fn conversation_times_follow_messages_and_resyncs_keep_the_order() {
    let (_, db) = fixture("times");
    let repo = Repository::new(&db);
    let timed = |id: &str, parent: Option<&str>, at: &str| {
        let mut m = message(id, parent, id);
        m.created_at = Some(at.to_string());
        m
    };
    let first = [
        timed("a", None, "2026-01-01T08:00:00Z"),
        // Antigravity-style timestamps are normalized too.
        timed("b", Some("a"), "2026-01-02 09:30:00.5+00:00"),
    ];
    let (id, _, _, _) = repo
        .sync_import_conversation("T", "test", "times", &first, None, None, "general")
        .unwrap();
    let conversation = repo.get_conversation(&id).unwrap().unwrap();
    assert_eq!(conversation.created_at.to_rfc3339(), "2026-01-01T08:00:00+00:00");
    assert_eq!(conversation.updated_at.to_rfc3339(), "2026-01-02T09:30:00.500+00:00");
    let messages = repo.list_messages(&id).unwrap();
    assert_eq!(messages[1].created_at.to_rfc3339(), "2026-01-02T09:30:00.500+00:00");

    // Scanning the same source again changes nothing.
    repo.sync_import_conversation("T", "test", "times", &first, None, None, "general")
        .unwrap();
    let unchanged = repo.get_conversation(&id).unwrap().unwrap();
    assert_eq!(unchanged.updated_at, conversation.updated_at);

    // A new message moves the conversation to its time.
    let mut second = first.to_vec();
    second.push(timed("c", Some("b"), "2026-01-03T10:00:00Z"));
    repo.sync_import_conversation("T", "test", "times", &second, None, None, "general")
        .unwrap();
    let moved = repo.get_conversation(&id).unwrap().unwrap();
    assert_eq!(moved.updated_at.to_rfc3339(), "2026-01-03T10:00:00+00:00");
}

#[test]
fn reset_clears_one_source_and_allows_reimport() {
    use context_vault_lib::persistence::ResetScope;
    use context_vault_lib::services::CodexRecoveryService;
    let (root, db) = fixture("reset");
    let home = codex_home(&root);
    let codex = CodexRecoveryService::new(&db, Some(&home));
    codex.recover(&codex.scan().candidates);
    let repo = Repository::new(&db);
    repo.sync_import_conversation(
        "Claude keeps",
        "claude_code",
        "session",
        &[message("m", None, "保留的 Claude 会话")],
        None,
        None,
        "claude",
    )
    .unwrap();
    // A deleted Codex conversation is excluded from later scans.
    let deleted = repo
        .list_conversations(false, None)
        .unwrap()
        .into_iter()
        .find(|c| c.source_kind == "codex_main")
        .unwrap();
    repo.delete_conversation(&deleted.id).unwrap();
    assert_eq!(codex.scan().candidates.len(), 1);

    let preview = repo
        .reset_conversations(ResetScope::Codex, true, true)
        .unwrap();
    assert_eq!(preview.conversation_count, 1);
    assert_eq!(preview.exclusion_count, 1);
    assert_eq!(repo.list_conversations(false, None).unwrap().len(), 2);

    let summary = repo
        .reset_conversations(ResetScope::Codex, true, false)
        .unwrap();
    assert_eq!(summary.conversation_count, 1);
    assert_eq!(summary.message_count, 2);
    let remaining = repo.list_conversations(false, None).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].source_kind, "claude");
    assert!(repo.search_conversations("question", 10).unwrap().is_empty());
    assert_eq!(repo.search_conversations("保留", 10).unwrap().len(), 1);

    // Both Codex threads, including the one deleted before, import again.
    let report = codex.scan();
    assert_eq!(report.candidates.len(), 2);
    assert!(report.candidates.iter().all(|c| !c.exists_locally));
    assert_eq!(codex.recover(&report.candidates).new_conversation_count, 2);
    db.lock_conn()
        .execute("INSERT INTO message_fts(message_fts) VALUES('integrity-check')", [])
        .unwrap();

    let all = repo.reset_conversations(ResetScope::All, false, false).unwrap();
    assert_eq!(all.conversation_count, 3);
    assert!(repo.list_conversations(false, None).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Imports

#[test]
fn chatgpt_paths_skip_tool_nodes_without_losing_earlier_messages() {
    let (root, db) = fixture("chatgpt-tools");
    let input = root.join("conversations.json");
    fs::write(&input, r#"[{"id":"tools","title":"Tools","current_node":"tool-final",
        "mapping":{
          "root":{"parent":null,"message":null},
          "sys":{"parent":"root","message":{"author":{"role":"system"},"content":{"parts":[""]},"metadata":{"is_visually_hidden_from_conversation":true}}},
          "q":{"parent":"sys","message":{"author":{"role":"user"},"content":{"parts":["Question"]},"create_time":1}},
          "call":{"parent":"q","message":{"author":{"role":"assistant"},"content":{"parts":["Calling tool"]},"create_time":2}},
          "tool":{"parent":"call","message":{"author":{"role":"tool"},"content":{"parts":["tool output"]},"create_time":3}},
          "a":{"parent":"tool","message":{"author":{"role":"assistant"},"content":{"parts":["Answer"]},"create_time":4}},
          "tool-final":{"parent":"a","message":{"author":{"role":"tool"},"content":{"parts":["ignored"]},"create_time":5}}
        }}]"#).unwrap();
    ImportService::new(&db).import_file(&input).unwrap();
    let repo = Repository::new(&db);
    let conversation = repo.list_conversations(false, None).unwrap().remove(0);
    let path = repo.load_active_path(&conversation.id).unwrap();
    assert_eq!(
        path.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
        ["Question", "Calling tool", "Answer"]
    );
}

#[test]
fn chatgpt_zip_prefers_conversations_json() {
    use std::io::Write;
    let (root, db) = fixture("chatgpt-zip");
    let archive = root.join("export.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    let options: zip::write::SimpleFileOptions = Default::default();
    zip.start_file("shared_conversations.json", options).unwrap();
    zip.write_all(br#"[{"id":"s","conversation_id":"x","title":"Shared"}]"#)
        .unwrap();
    zip.start_file("conversations.json", options).unwrap();
    zip.write_all(br#"[{"id":"c","title":"Real","current_node":"m","mapping":{"m":{"parent":null,"message":{"author":{"role":"user"},"content":{"parts":["Zip content"]}}}}}]"#).unwrap();
    zip.finish().unwrap();
    let result = ImportService::new(&db).import_file(&archive).unwrap();
    assert_eq!(result.new_conversation_count, 1);
    assert_eq!(result.message_count, 1);
}

// ---------------------------------------------------------------------------
// Claude and Antigravity

#[test]
fn claude_project_folders_follow_claude_code_naming() {
    assert_eq!(
        ClaudePublisher::project_dir_name(
            r"G:\tools\Context restoration app\Context restoration app - ts_rust"
        ),
        "G--tools-Context-restoration-app-Context-restoration-app---ts-rust"
    );
    assert_eq!(
        ClaudePublisher::project_dir_name(r"G:\web\mold large model web"),
        "G--web-mold-large-model-web"
    );
}

#[test]
fn claude_scan_reads_transcript_titles_and_tool_output_lists() {
    use context_vault_lib::services::ClaudeRecoveryService;
    let (root, db) = fixture("claude-transcript");
    let home = root.join("claude");
    let project = home.join("projects/G--demo");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("session-1.jsonl"),
        concat!(
            "{\"type\":\"user\",\"isMeta\":true,\"uuid\":\"meta\",\"message\":{\"role\":\"user\",\"content\":\"Caveat: injected\"},\"cwd\":\"G:\\\\demo\"}\n",
            "{\"type\":\"user\",\"uuid\":\"u1\",\"message\":{\"role\":\"user\",\"content\":\"Real question\"},\"cwd\":\"G:\\\\demo\"}\n",
            "{\"type\":\"assistant\",\"uuid\":\"a1\",\"requestId\":\"r1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{\"command\":\"ls\"}}]}}\n",
            "{\"type\":\"user\",\"uuid\":\"t1\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"content\":[{\"type\":\"text\",\"text\":\"file.txt\"}]}]}}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"Old name\",\"sessionId\":\"session-1\"}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"Renamed session\",\"sessionId\":\"session-1\"}\n",
        ),
    )
    .unwrap();
    let report = ClaudeRecoveryService::new(&db, Some(&home)).scan();
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].title, "Renamed session");
    assert_eq!(report.candidates[0].preview, "Real question");
    let messages =
        ClaudeRecoveryService::parse_session_file(&project.join("session-1.jsonl"), "session-1");
    assert!(messages.iter().all(|m| !m.content.contains("Caveat")));
    assert!(messages.iter().any(|m| m.content.contains("file.txt")));
}

#[test]
fn claude_publish_writes_new_sessions_where_claude_code_looks() {
    let (root, db) = fixture("claude-new-session");
    let repo = Repository::new(&db);
    let (id, _, _, _) = repo
        .sync_import_conversation(
            "Imported elsewhere",
            "claude_code",
            "fresh-session",
            &[message("m1", None, "Hello")],
            None,
            Some(r"G:\web\demo app"),
            "claude",
        )
        .unwrap();
    let home = root.join("claude");
    ClaudePublisher::new(&db, Some(&home), &root.join("backups"))
        .publish(&[id], false)
        .unwrap();
    let transcript = home.join("projects/G--web-demo-app/fresh-session.jsonl");
    let text = fs::read_to_string(&transcript).unwrap();
    let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(last["type"], "custom-title");
    assert_eq!(last["customTitle"], "Imported elsewhere");
}

#[test]
fn antigravity_workspace_uris_round_trip_unicode_paths() {
    use context_vault_lib::services::AntigravityRecoveryService;
    let encoded = AntigravityPublisher::encode_workspace_uri(Some(r"G:\工作\update ui_0326"));
    assert_eq!(
        encoded,
        r#"["file:///g%3A/%E5%B7%A5%E4%BD%9C/update%20ui_0326"]"#
    );
    let decoded = AntigravityRecoveryService::decode_workspace_uri(&encoded).unwrap();
    assert_eq!(decoded, r"G:\工作\update ui_0326");
}

#[test]
fn antigravity_scan_keeps_rows_with_null_columns() {
    use context_vault_lib::services::AntigravityRecoveryService;
    let (root, db) = fixture("ag-null");
    let home = root.join("antigravity");
    let logs = home.join("brain/conv-1/.system_generated/logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(
        logs.join("transcript.jsonl"),
        "{\"step_index\":0,\"type\":\"USER_INPUT\",\"content\":\"<USER_REQUEST>hello</USER_REQUEST>\"}\n",
    )
    .unwrap();
    let summaries = rusqlite::Connection::open(home.join("conversation_summaries.db")).unwrap();
    summaries
        .execute_batch(
            "CREATE TABLE conversation_summaries (conversation_id TEXT, title TEXT, preview TEXT,
                step_count INTEGER, last_modified_time datetime, workspace_uris TEXT, status TEXT,
                nesting_depth INTEGER, parent_conversation_id TEXT);
             INSERT INTO conversation_summaries VALUES ('conv-1', 'Indexed title', NULL, 2,
                '2026-02-02 08:54:59.6892244+00:00', '[\"file:///g%3A/demo\"]', 'IDLE', 0, NULL);",
        )
        .unwrap();
    let report = AntigravityRecoveryService::new(&db, Some(&home)).scan();
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].title, "Indexed title");
    assert_eq!(
        report.candidates[0].last_modified_time,
        "2026-02-02T08:54:59.689224400+00:00"
    );
}
