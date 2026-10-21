use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, Result};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::database::Database;
use crate::util::normalize_timestamp;
use crate::domain::{
    CanonicalMessage, Conversation, ConversationSummary, MessageRole, MessageStatus,
};

pub struct Repository<'a> {
    db: &'a Database,
}

/// Which conversations a vault reset removes; matches the source tabs of the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetScope {
    All,
    /// Everything shown in the Codex tab, which includes file imports.
    Codex,
    Antigravity,
    Claude,
}

impl ResetScope {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "all" => Some(Self::All),
            "codex" => Some(Self::Codex),
            "antigravity" => Some(Self::Antigravity),
            "claude" => Some(Self::Claude),
            _ => None,
        }
    }

    fn conversation_filter(self) -> &'static str {
        match self {
            Self::All => "1 = 1",
            Self::Codex => "source_kind NOT IN ('antigravity', 'claude')",
            Self::Antigravity => "source_kind = 'antigravity'",
            Self::Claude => "source_kind = 'claude'",
        }
    }

    fn exclusion_filter(self) -> &'static str {
        match self {
            Self::All => "1 = 1",
            Self::Codex => "source_type NOT IN ('antigravity_transcript', 'claude_code')",
            Self::Antigravity => "source_type = 'antigravity_transcript'",
            Self::Claude => "source_type = 'claude_code'",
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ResetSummary {
    pub conversation_count: i64,
    pub message_count: i64,
    pub exclusion_count: i64,
    pub folder_pin_count: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FolderPin {
    pub source: String,
    pub path: String,
    pub pinned_at: String,
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn parse_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .or_else(|_| DateTime::parse_from_rfc3339(&normalize_timestamp(s)))
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

/// Bounds for `source_external_id` values that start with `<source_type>:<source_id>:`.
/// A range comparison can use the unique index, unlike `LIKE` or `substr`, and does
/// not treat `_` or `%` inside IDs as wildcards.
fn source_prefix_range(source_type: &str, source_id: &str) -> (String, String) {
    (
        format!("{}:{}:", source_type, source_id),
        format!("{}:{};", source_type, source_id),
    )
}

/// Extracts the client conversation ID from `<source_type>:<source_id>:<message>`.
pub fn source_id_from_external_id(source_type: &str, external_id: &str) -> Option<String> {
    let rest = external_id.strip_prefix(source_type)?.strip_prefix(':')?;
    let id = rest.split(':').next()?.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// Wraps a search term for `LIKE ... ESCAPE '\'`.
fn like_pattern(term: &str) -> String {
    let mut pattern = String::with_capacity(term.len() + 2);
    pattern.push('%');
    for c in term.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> Result<ConversationSummary> {
    Ok(ConversationSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        source_title: row.get(2)?,
        title_user_edited: row.get::<_, i64>(3)? != 0,
        project_path: row.get(4)?,
        source_kind: row.get(5)?,
        archived: row.get::<_, i64>(6)? != 0,
        message_count: row.get(7)?,
        created_at: parse_dt(&row.get::<_, String>(8)?),
        updated_at: parse_dt(&row.get::<_, String>(9)?),
        pinned_at: row.get(10)?,
    })
}

impl<'a> Repository<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn list_folder_pins(&self) -> Result<Vec<FolderPin>> {
        let conn = self.db.lock_conn();
        let mut statement = conn.prepare("SELECT source, path, pinned_at FROM folder_pins ORDER BY pinned_at DESC, path")?;
        let rows = statement.query_map([], |row| Ok(FolderPin {
            source: row.get(0)?, path: row.get(1)?, pinned_at: row.get(2)?,
        }))?;
        rows.collect()
    }

    pub fn set_folder_pinned(&self, source: &str, path: &str, pinned: bool) -> Result<()> {
        let path = crate::util::normalize_windows_path(path)
            .replace('/', "\\").trim_end_matches('\\').to_lowercase();
        if path.is_empty() || !matches!(source, "codex" | "antigravity" | "claude") {
            return Err(rusqlite::Error::InvalidParameterName("Invalid folder pin".into()));
        }
        let conn = self.db.lock_conn();
        if pinned {
            conn.execute(
                "INSERT INTO folder_pins(source, path, pinned_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(source, path) DO NOTHING",
                params![source, path, now_iso()],
            )?;
        } else {
            conn.execute("DELETE FROM folder_pins WHERE source = ?1 AND path = ?2", params![source, path])?;
        }
        Ok(())
    }

    pub fn list_conversations(
        &self,
        archived: bool,
        limit: Option<i64>,
    ) -> Result<Vec<Conversation>> {
        let conn = self.db.lock_conn();
        let sql = format!(
            "SELECT id, title, source_title, title_user_edited, system_prompt,
                    active_provider_id, active_model, active_leaf_message_id,
                    project_path, source_kind, archived, created_at, updated_at, pinned_at
             FROM conversations
             WHERE archived = ?1
             ORDER BY pinned_at DESC, updated_at DESC
             {}",
            limit.map(|l| format!("LIMIT {}", l)).unwrap_or_default()
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([archived as i64], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                title: row.get(1)?,
                source_title: row.get(2)?,
                title_user_edited: row.get::<_, i64>(3)? != 0,
                system_prompt: row.get(4)?,
                active_provider_id: row.get(5)?,
                active_model: row.get(6)?,
                active_leaf_message_id: row.get(7)?,
                project_path: row.get(8)?,
                source_kind: row.get(9)?,
                archived: row.get::<_, i64>(10)? != 0,
                created_at: parse_dt(&row.get::<_, String>(11)?),
                updated_at: parse_dt(&row.get::<_, String>(12)?),
                pinned_at: row.get(13)?,
            })
        })?;

        let mut res = Vec::new();
        for r in rows {
            res.push(r?);
        }
        Ok(res)
    }

    pub fn list_conversation_summaries(
        &self,
        archived: bool,
        limit: Option<i64>,
    ) -> Result<Vec<ConversationSummary>> {
        let conn = self.db.lock_conn();
        let sql = format!(
            "SELECT c.id, c.title, c.source_title, c.title_user_edited,
                    c.project_path, c.source_kind, c.archived,
                    COUNT(m.id) AS msg_count, c.created_at, c.updated_at, c.pinned_at
             FROM conversations c
             LEFT JOIN messages m ON m.conversation_id = c.id
             WHERE c.archived = ?1
             GROUP BY c.id
             ORDER BY c.pinned_at DESC, c.updated_at DESC
             {}",
            limit.map(|l| format!("LIMIT {}", l)).unwrap_or_default()
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([archived as i64], summary_from_row)?;

        let mut res = Vec::new();
        for r in rows {
            res.push(r?);
        }
        Ok(res)
    }

    pub fn get_conversation(&self, id: &str) -> Result<Option<Conversation>> {
        let conn = self.db.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, title, source_title, title_user_edited, system_prompt,
                    active_provider_id, active_model, active_leaf_message_id,
                    project_path, source_kind, archived, created_at, updated_at, pinned_at
             FROM conversations WHERE id = ?",
        )?;

        let mut rows = stmt.query_map([id], |row| {
            Ok(Conversation {
                id: row.get(0)?,
                title: row.get(1)?,
                source_title: row.get(2)?,
                title_user_edited: row.get::<_, i64>(3)? != 0,
                system_prompt: row.get(4)?,
                active_provider_id: row.get(5)?,
                active_model: row.get(6)?,
                active_leaf_message_id: row.get(7)?,
                project_path: row.get(8)?,
                source_kind: row.get(9)?,
                archived: row.get::<_, i64>(10)? != 0,
                created_at: parse_dt(&row.get::<_, String>(11)?),
                updated_at: parse_dt(&row.get::<_, String>(12)?),
                pinned_at: row.get(13)?,
            })
        })?;

        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
    }

    pub fn rename_conversation(&self, id: &str, new_title: &str) -> Result<bool> {
        let conn = self.db.lock_conn();
        let now = now_iso();
        let rows = conn.execute(
            "UPDATE conversations
             SET title = ?1, title_user_edited = 1, updated_at = ?2
             WHERE id = ?3",
            params![new_title, now, id],
        )?;
        Ok(rows > 0)
    }

    pub fn set_archived(&self, id: &str, archived: bool) -> Result<bool> {
        let conn = self.db.lock_conn();
        let now = now_iso();
        let rows = conn.execute(
            "UPDATE conversations SET archived = ?1, updated_at = ?2 WHERE id = ?3",
            params![archived as i64, now, id],
        )?;
        Ok(rows > 0)
    }

    /// Pinning is local metadata; it never changes message recency or import IDs.
    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<bool> {
        let conn = self.db.lock_conn();
        let rows = conn.execute(
            "UPDATE conversations
             SET pinned_at = CASE WHEN ?1 THEN COALESCE(pinned_at, ?2) ELSE NULL END
             WHERE id = ?3",
            params![pinned, now_iso(), id],
        )?;
        Ok(rows > 0)
    }

    /// Counts, and unless `dry_run` removes, the conversations of `scope`. Unlike
    /// `delete_conversations` this records no recovery exclusions, so the removed
    /// conversations can be scanned and imported again. With `clear_exclusions`,
    /// conversations deleted earlier become importable again too.
    pub fn reset_conversations(
        &self,
        scope: ResetScope,
        clear_exclusions: bool,
        dry_run: bool,
    ) -> Result<ResetSummary> {
        let conversations = scope.conversation_filter();
        let exclusions = scope.exclusion_filter();
        let pin_filter = match scope {
            ResetScope::All => "1 = 1",
            ResetScope::Codex => "source = 'codex'",
            ResetScope::Antigravity => "source = 'antigravity'",
            ResetScope::Claude => "source = 'claude'",
        };
        let mut connection = self.db.lock_conn();
        let transaction = connection.transaction()?;
        let count = |sql: &str| -> Result<i64> { transaction.query_row(sql, [], |r| r.get(0)) };
        let summary = ResetSummary {
            conversation_count: count(&format!(
                "SELECT COUNT(*) FROM conversations WHERE {conversations}"
            ))?,
            message_count: count(&format!(
                "SELECT COUNT(*) FROM messages
                 WHERE conversation_id IN (SELECT id FROM conversations WHERE {conversations})"
            ))?,
            exclusion_count: count(&format!(
                "SELECT COUNT(*) FROM recovery_exclusions WHERE {exclusions}"
            ))?,
            folder_pin_count: count(&format!("SELECT COUNT(*) FROM folder_pins WHERE {pin_filter}"))?,
        };
        if dry_run {
            return Ok(summary);
        }
        // Messages, request attempts and checkpoints are removed by ON DELETE CASCADE;
        // the message triggers keep the search index in step.
        transaction.execute(
            &format!("DELETE FROM conversations WHERE {conversations}"),
            [],
        )?;
        transaction.execute(&format!("DELETE FROM folder_pins WHERE {pin_filter}"), [])?;
        if clear_exclusions {
            transaction.execute(
                &format!("DELETE FROM recovery_exclusions WHERE {exclusions}"),
                [],
            )?;
        }
        transaction.commit()?;
        Ok(ResetSummary {
            exclusion_count: if clear_exclusions { summary.exclusion_count } else { 0 },
            ..summary
        })
    }

    pub fn delete_conversation(&self, id: &str) -> Result<bool> {
        self.delete_conversations(&[id.to_string()]).map(|n| n > 0)
    }

    pub fn delete_conversations(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut connection = self.db.lock_conn();
        let transaction = connection.transaction()?;
        let conn = &transaction;
        let now = now_iso();

        let mut deleted = 0;
        for id in ids {
            // Check if there are external source messages to record as exclusions
            let mut stmt = conn.prepare(
                "SELECT c.title, c.project_path, m.source_type, m.source_external_id
                 FROM conversations c
                 JOIN messages m ON m.conversation_id = c.id
                 WHERE c.id = ? AND m.source_external_id IS NOT NULL",
            )?;

            let rows: Vec<(String, Option<String>, String, String)> = stmt
                .query_map([id], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })?
                .filter_map(|r| r.ok())
                .collect();

            let mut exclusions = HashSet::new();
            for (title, project_path, source_type, ext_id) in rows {
                if let Some(source_conv_id) = source_id_from_external_id(&source_type, &ext_id) {
                    exclusions.insert((source_type, source_conv_id, title, project_path));
                }
            }

            for (st, scid, title, proj) in exclusions {
                conn.execute(
                    "INSERT INTO recovery_exclusions(source_type, source_conversation_id, title, project_path, excluded_at)
                     VALUES(?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(source_type, source_conversation_id) DO UPDATE SET
                        title = excluded.title, project_path = excluded.project_path, excluded_at = excluded.excluded_at",
                    params![st, scid, title, proj, now],
                )?;
            }

            deleted += conn.execute("DELETE FROM conversations WHERE id = ?", [id])?;
        }

        transaction.commit()?;
        Ok(deleted)
    }

    pub fn list_messages(&self, conversation_id: &str) -> Result<Vec<CanonicalMessage>> {
        let conn = self.db.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, parent_message_id, sequence_no,
                    role, content, status, provider_id, model,
                    remote_response_id, source_type, source_external_id,
                    created_at, updated_at
             FROM messages
             WHERE conversation_id = ?
             ORDER BY sequence_no ASC",
        )?;

        let rows = stmt.query_map([conversation_id], |row| {
            let role_str: String = row.get(4)?;
            let status_str: String = row.get(6)?;
            Ok(CanonicalMessage {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                parent_message_id: row.get(2)?,
                sequence_no: row.get(3)?,
                role: MessageRole::from_str(&role_str),
                content: row.get(5)?,
                status: MessageStatus::from_str(&status_str),
                provider_id: row.get(7)?,
                model: row.get(8)?,
                remote_response_id: row.get(9)?,
                source_type: row.get(10)?,
                source_external_id: row.get(11)?,
                created_at: parse_dt(&row.get::<_, String>(12)?),
                updated_at: parse_dt(&row.get::<_, String>(13)?),
            })
        })?;

        let mut msgs = Vec::new();
        for r in rows {
            msgs.push(r?);
        }
        Ok(msgs)
    }

    pub fn load_active_path(&self, conversation_id: &str) -> Result<Vec<CanonicalMessage>> {
        let msgs = self.list_messages(conversation_id)?;
        let conv = self.get_conversation(conversation_id)?;
        let active_leaf_id = conv.and_then(|c| c.active_leaf_message_id);

        if let Some(leaf_id) = active_leaf_id {
            let map: HashMap<String, &CanonicalMessage> =
                msgs.iter().map(|m| (m.id.clone(), m)).collect();
            let mut path = Vec::new();
            let mut current = Some(leaf_id.as_str());
            let mut visited = HashSet::new();

            while let Some(cid) = current {
                if !visited.insert(cid) {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                if let Some(msg) = map.get(cid) {
                    path.push((*msg).clone());
                    current = msg.parent_message_id.as_deref();
                } else {
                    break;
                }
            }
            path.reverse();
            if !path.is_empty() {
                return Ok(path);
            }
        }
        Ok(msgs)
    }

    pub fn search_conversations(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<ConversationSummary>> {
        self.search_conversations_with_archived(query, limit, false)
    }

    pub fn search_conversations_with_archived(
        &self,
        query: &str,
        limit: i64,
        include_archived: bool,
    ) -> Result<Vec<ConversationSummary>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            let mut list = self.list_conversation_summaries(false, None)?;
            if include_archived {
                list.extend(self.list_conversation_summaries(true, None)?);
            }
            list.sort_by(|a, b| b.pinned_at.cmp(&a.pinned_at).then(b.updated_at.cmp(&a.updated_at)));
            if limit >= 0 {
                list.truncate(limit as usize);
            }
            return Ok(list);
        }

        // Every whitespace-separated term must occur in the title or in a message.
        // Terms of three or more characters use the trigram index; shorter terms,
        // which are common in Chinese, fall back to LIKE.
        let mut sql = String::from(
            "SELECT c.id, c.title, c.source_title, c.title_user_edited,
                    c.project_path, c.source_kind, c.archived,
                    (SELECT COUNT(*) FROM messages WHERE conversation_id = c.id) AS msg_count,
                    c.created_at, c.updated_at, c.pinned_at
             FROM conversations c
             WHERE (?1 OR c.archived = 0)",
        );
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(include_archived)];
        for term in trimmed.split_whitespace() {
            values.push(Box::new(like_pattern(term)));
            let like = values.len();
            if term.chars().count() >= 3 {
                values.push(Box::new(format!("\"{}\"", term.replace('"', "\"\""))));
                let fts = values.len();
                sql.push_str(&format!(
                    " AND (c.title LIKE ?{like} ESCAPE '\\' OR c.id IN (
                        SELECT m.conversation_id FROM message_fts
                        JOIN messages m ON m.rowid = message_fts.rowid
                        WHERE message_fts MATCH ?{fts}))"
                ));
            } else {
                sql.push_str(&format!(
                    " AND (c.title LIKE ?{like} ESCAPE '\\' OR EXISTS (
                        SELECT 1 FROM messages m
                        WHERE m.conversation_id = c.id AND m.content LIKE ?{like} ESCAPE '\\'))"
                ));
            }
        }
        values.push(Box::new(limit));
        sql.push_str(&format!(" ORDER BY c.pinned_at DESC, c.updated_at DESC LIMIT ?{}", values.len()));

        let conn = self.db.lock_conn();
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
        let rows = stmt.query_map(params.as_slice(), summary_from_row)?;
        rows.collect()
    }

    /// Whether a source conversation has a message whose own ID starts with `message_prefix`.
    pub fn has_source_message_prefix(
        &self,
        source_type: &str,
        source_id: &str,
        message_prefix: &str,
    ) -> Result<bool> {
        let lower = format!("{}:{}:{}", source_type, source_id, message_prefix);
        let upper = format!("{}\u{10FFFF}", lower);
        let conn = self.db.lock_conn();
        conn.query_row(
            "SELECT 1 FROM messages
             WHERE source_type = ?1 AND source_external_id >= ?2 AND source_external_id < ?3
             LIMIT 1",
            params![source_type, lower, upper],
            |_| Ok(()),
        )
        .optional()
        .map(|found| found.is_some())
    }

    /// Maps vault conversation IDs to the IDs the source client uses for them.
    /// Conversations that did not come from `source_type` are left out.
    pub fn source_ids_for_conversations(
        &self,
        source_type: &str,
        conversation_ids: &[String],
    ) -> Result<HashMap<String, String>> {
        let conn = self.db.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT source_external_id FROM messages
             WHERE conversation_id = ?1 AND source_type = ?2 AND source_external_id IS NOT NULL
             LIMIT 1",
        )?;
        let mut mapping = HashMap::new();
        for id in conversation_ids {
            let external: Option<String> = stmt
                .query_row(params![id, source_type], |r| r.get(0))
                .optional()?;
            if let Some(source_id) =
                external.and_then(|e| source_id_from_external_id(source_type, &e))
            {
                mapping.insert(id.clone(), source_id);
            }
        }
        Ok(mapping)
    }

    /// Maps every vault conversation imported from `source_type` to the ID the source
    /// client uses for it.
    pub fn source_ids_by_conversation(&self, source_type: &str) -> Result<HashMap<String, String>> {
        let conn = self.db.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT conversation_id, MIN(source_external_id) FROM messages
             WHERE source_type = ?1 AND source_external_id IS NOT NULL
             GROUP BY conversation_id",
        )?;
        let rows = stmt.query_map([source_type], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut mapping = HashMap::new();
        for r in rows {
            let (conversation_id, external) = r?;
            if let Some(source_id) = source_id_from_external_id(source_type, &external) {
                mapping.insert(conversation_id, source_id);
            }
        }
        Ok(mapping)
    }

    pub fn excluded_source_ids(&self, source_type: &str) -> Result<HashSet<String>> {
        let conn = self.db.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT source_conversation_id FROM recovery_exclusions WHERE source_type = ?",
        )?;
        let rows = stmt.query_map([source_type], |row| row.get(0))?;
        let mut res = HashSet::new();
        for r in rows {
            res.insert(r?);
        }
        Ok(res)
    }

    pub fn imported_source_ids(&self, source_type: &str) -> Result<HashSet<String>> {
        let conn = self.db.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT source_external_id FROM messages
             WHERE source_type = ? AND source_external_id IS NOT NULL",
        )?;
        let rows = stmt.query_map([source_type], |row| row.get::<_, String>(0))?;

        let mut res = HashSet::new();
        for r in rows {
            if let Some(id) = source_id_from_external_id(source_type, &r?) {
                res.insert(id);
            }
        }
        Ok(res)
    }

    pub fn remember_recovery_exclusions(
        &self,
        source_type: &str,
        records: &[(String, String, Option<String>)],
    ) -> Result<usize> {
        if records.is_empty() {
            return Ok(0);
        }
        let mut connection = self.db.lock_conn();
        let transaction = connection.transaction()?;
        let now = now_iso();
        let mut count = 0;
        for (sid, title, project_path) in records {
            count += transaction.execute(
                "INSERT INTO recovery_exclusions(source_type, source_conversation_id, title, project_path, excluded_at)
                 VALUES(?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(source_type, source_conversation_id) DO UPDATE SET
                    title = excluded.title, project_path = excluded.project_path, excluded_at = excluded.excluded_at",
                params![source_type, sid, title, project_path, now],
            )?;
        }
        transaction.commit()?;
        Ok(count)
    }

    pub fn sync_import_titles(
        &self,
        source_type: &str,
        titles: &HashMap<String, String>,
    ) -> Result<usize> {
        let mut connection = self.db.lock_conn();
        let transaction = connection.transaction()?;
        let conn = &transaction;
        let now = now_iso();
        let mut updated_count = 0;

        for (source_id, source_title) in titles {
            let (lower, upper) = source_prefix_range(source_type, source_id);
            let mut stmt = conn.prepare_cached(
                "SELECT DISTINCT conversation_id FROM messages
                 WHERE source_type = ?1 AND source_external_id >= ?2 AND source_external_id < ?3",
            )?;

            let conv_ids: Vec<String> = stmt
                .query_map(params![source_type, lower, upper], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            for conv_id in conv_ids {
                let mut cstmt = conn.prepare(
                    "SELECT title, source_title, title_user_edited FROM conversations WHERE id = ?",
                )?;
                let cdata = cstmt.query_row([&conv_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, i64>(2)? != 0,
                    ))
                });

                if let Ok((current_title, prev_source_title, mut user_edited)) = cdata {
                    if prev_source_title.is_none() && current_title != *source_title {
                        user_edited = true;
                    }
                    let visible_title = if user_edited {
                        current_title.clone()
                    } else {
                        source_title.clone()
                    };

                    let title_changed = visible_title != current_title;
                    let rows = conn.execute(
                        "UPDATE conversations
                         SET title = ?1, source_title = ?2, title_user_edited = ?3,
                             updated_at = CASE WHEN title <> ?1 THEN ?4 ELSE updated_at END
                         WHERE id = ?5",
                        params![
                            visible_title,
                            source_title,
                            user_edited as i64,
                            now,
                            conv_id
                        ],
                    )?;
                    if rows > 0 && title_changed {
                        updated_count += 1;
                    }
                }
            }
        }
        transaction.commit()?;
        Ok(updated_count)
    }

    pub fn update_import_metadata(
        &self,
        source_type: &str,
        metadata: &HashMap<String, (Option<String>, String)>,
    ) -> Result<usize> {
        let mut connection = self.db.lock_conn();
        let transaction = connection.transaction()?;
        let mut count = 0;

        for (source_id, (project_path, source_kind)) in metadata {
            let (lower, upper) = source_prefix_range(source_type, source_id);
            count += transaction.execute(
                "UPDATE conversations
                 SET project_path = COALESCE(project_path, ?1),
                     source_kind = ?2
                 WHERE id IN (
                     SELECT DISTINCT conversation_id FROM messages
                     WHERE source_type = ?3
                       AND source_external_id >= ?4 AND source_external_id < ?5
                 )",
                params![project_path, source_kind, source_type, lower, upper],
            )?;
        }
        transaction.commit()?;
        Ok(count)
    }

    pub fn sync_import_conversation(
        &self,
        title: &str,
        source_type: &str,
        source_conversation_id: &str,
        messages: &[ImportedMessagePayload],
        active_external_id: Option<&str>,
        project_path: Option<&str>,
        source_kind: &str,
    ) -> Result<(String, usize, bool, bool)> {
        let mut connection = self.db.lock_conn();
        let transaction = connection.transaction()?;
        let conn = &transaction;
        let now = now_iso();

        // Conversation times follow the source messages rather than the import time,
        // so the list is ordered by when a conversation actually took place.
        let message_times: Vec<Option<DateTime<Utc>>> = messages
            .iter()
            .map(|m| {
                m.created_at
                    .as_deref()
                    .map(normalize_timestamp)
                    .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                    .map(|t| t.with_timezone(&Utc))
            })
            .collect();
        let earliest = message_times.iter().flatten().min().map(|t| t.to_rfc3339());
        let latest = message_times.iter().flatten().max().map(|t| t.to_rfc3339());

        let (lower, upper) = source_prefix_range(source_type, source_conversation_id);
        let existing_id: Option<String> = conn
            .query_row(
                "SELECT conversation_id FROM messages
                 WHERE source_type = ?1 AND source_external_id >= ?2 AND source_external_id < ?3
                 LIMIT 1",
                params![source_type, lower, upper],
                |row| row.get(0),
            )
            .optional()?;

        let (conv_id, created) = if let Some(id) = existing_id {
            (id, false)
        } else {
            let new_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO conversations(
                    id, title, source_title, title_user_edited, project_path,
                    source_kind, created_at, updated_at
                ) VALUES(?1, ?2, ?3, 0, ?4, ?5, ?6, ?7)",
                params![
                    new_id,
                    title,
                    title,
                    project_path,
                    source_kind,
                    earliest.as_deref().unwrap_or(&now),
                    latest.as_deref().unwrap_or(&now)
                ],
            )?;
            (new_id, true)
        };

        // Sync title if existing
        let mut title_updated = false;
        if !created {
            let mut cstmt = conn.prepare(
                "SELECT title, source_title, title_user_edited FROM conversations WHERE id = ?",
            )?;
            if let Ok((cur_title, prev_src_title, mut user_edited)) =
                cstmt.query_row([&conv_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, i64>(2)? != 0,
                    ))
                })
            {
                if prev_src_title.is_none() && cur_title != title {
                    user_edited = true;
                }
                let visible_title = if user_edited {
                    cur_title.clone()
                } else {
                    title.to_string()
                };
                title_updated = visible_title != cur_title;

                conn.execute(
                    "UPDATE conversations
                     SET title = ?1, source_title = ?2, title_user_edited = ?3,
                         project_path = COALESCE(project_path, ?4),
                         source_kind = ?5
                     WHERE id = ?6",
                    params![
                        visible_title,
                        title,
                        user_edited as i64,
                        project_path,
                        source_kind,
                        conv_id
                    ],
                )?;
            }
        }

        // Insert new messages
        let mut existing_ext_ids: HashMap<String, String> = {
            let mut stmt = conn.prepare(
                "SELECT source_external_id, id FROM messages
                 WHERE source_type = ?1 AND conversation_id = ?2 AND source_external_id IS NOT NULL",
            )?;
            let rows = stmt.query_map(params![source_type, conv_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
            rows.collect::<Result<HashMap<String, String>>>()?
        };

        let mut current_seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sequence_no), 0) FROM messages WHERE conversation_id = ?",
            [&conv_id],
            |r| r.get(0),
        )?;

        let mut inserted_count = 0;
        let mut active_leaf_db_id = None;

        for (msg, time) in messages.iter().zip(&message_times) {
            let formatted_ext_id = format!(
                "{}:{}:{}",
                source_type, source_conversation_id, msg.external_id
            );

            let db_msg_id = if let Some(existing_id) = existing_ext_ids.get(&formatted_ext_id) {
                conn.execute(
                    "UPDATE messages SET content = ?1, role = ?2, updated_at = ?3
                     WHERE id = ?4 AND (content <> ?1 OR role <> ?2)",
                    params![msg.content, msg.role, now, existing_id],
                )?;
                existing_id.clone()
            } else {
                current_seq += 1;
                let new_msg_id = Uuid::new_v4().to_string();
                let parent_db_id: Option<String> = None;

                let msg_created_at = time.map(|t| t.to_rfc3339()).unwrap_or_else(|| now.clone());

                conn.execute(
                    "INSERT INTO messages(
                        id, conversation_id, parent_message_id, sequence_no,
                        role, content, status, source_type, source_external_id,
                        created_at, updated_at
                    ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'complete', ?7, ?8, ?9, ?10)",
                    params![
                        new_msg_id,
                        conv_id,
                        parent_db_id,
                        current_seq,
                        msg.role,
                        msg.content,
                        source_type,
                        formatted_ext_id,
                        msg_created_at,
                        now
                    ],
                )?;

                existing_ext_ids.insert(formatted_ext_id.clone(), new_msg_id.clone());
                inserted_count += 1;
                new_msg_id
            };

            if let Some(target_leaf) = active_external_id {
                if msg.external_id == target_leaf {
                    active_leaf_db_id = Some(db_msg_id);
                }
            } else {
                active_leaf_db_id = Some(db_msg_id);
            }
        }

        // All IDs now exist, so link parents independently of input order.
        // This also repairs parent links on conversations imported by older builds.
        for msg in messages {
            let ext_id = format!(
                "{}:{}:{}",
                source_type, source_conversation_id, msg.external_id
            );
            let parent_id = msg.parent_external_id.as_ref().and_then(|parent| {
                existing_ext_ids.get(&format!(
                    "{}:{}:{}",
                    source_type, source_conversation_id, parent
                ))
            });
            conn.execute(
                "UPDATE messages SET parent_message_id = ?1
                 WHERE id = ?2 AND parent_message_id IS NOT ?1",
                params![parent_id, existing_ext_ids[&ext_id]],
            )?;
        }

        if let Some(leaf_id) = active_leaf_db_id {
            conn.execute(
                "UPDATE conversations SET active_leaf_message_id = ?1 WHERE id = ?2",
                params![leaf_id, conv_id],
            )?;
        }
        // Only new messages move a conversation; re-scanning unchanged sources must
        // not reorder the list.
        if !created && inserted_count > 0 {
            let newest = latest.clone().unwrap_or_else(|| now.clone());
            conn.execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2 AND updated_at < ?1",
                params![newest, conv_id],
            )?;
        }

        transaction.commit()?;
        Ok((conv_id, inserted_count, created, title_updated))
    }

    pub fn recover_stale_requests(&self) -> Result<(usize, usize)> {
        let conn = self.db.lock_conn();
        let now = now_iso();

        let updated_msgs = conn.execute(
            "UPDATE messages SET status = 'interrupted', updated_at = ?
             WHERE status = 'streaming'",
            [&now],
        )?;

        let updated_attempts = conn.execute(
            "UPDATE request_attempts
             SET status = 'failed', error_category = 'crash_recovery',
                 error_message = '应用异常退出，请求已恢复为中断状态',
                 finished_at = ?
             WHERE status IN ('queued', 'running')",
            [&now],
        )?;

        Ok((updated_msgs, updated_attempts))
    }

    pub fn export_conversation(&self, conversation_id: &str) -> Result<serde_json::Value> {
        let conv = self.get_conversation(conversation_id)?;
        let msgs = self.list_messages(conversation_id)?;
        Ok(serde_json::json!({
            "conversation": conv,
            "messages": msgs,
        }))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportedMessagePayload {
    pub role: String,
    pub content: String,
    pub external_id: String,
    pub parent_external_id: Option<String>,
    pub created_at: Option<String>,
}
