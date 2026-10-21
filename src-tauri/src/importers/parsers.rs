use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

use super::models::{ImportedConversation, ImportedMessage};

pub fn probe_format(path: &Path) -> Result<String, String> {
    if let Ok(file) = File::open(path) {
        if ZipArchive::new(file).is_ok() {
            return Ok("chatgpt_export".to_string());
        }
    }

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext == "md" || ext == "markdown" {
        return Ok("markdown".to_string());
    }

    if ext == "json" || ext == "jsonl" || ext == "ndjson" {
        if let Ok(mut file) = File::open(path) {
            let mut buf = [0u8; 1024];
            let read_bytes = file.read(&mut buf).unwrap_or(0);
            let snippet = String::from_utf8_lossy(&buf[..read_bytes]);
            if snippet.contains("\"mapping\"") && snippet.contains("\"current_node\"") {
                return Ok("chatgpt_export".to_string());
            }
        }
        return Ok("openai_messages".to_string());
    }

    Err(format!("不支持的文件类型：{}", ext))
}

pub fn parse_file(path: &Path, source_type: &str) -> Result<Vec<ImportedConversation>, String> {
    match source_type {
        "markdown" => {
            let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
            let fallback_title = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Markdown 导入");
            Ok(parse_markdown(&text, fallback_title))
        }
        "chatgpt_export" => parse_chatgpt(path),
        _ => parse_openai_messages(path),
    }
}

pub fn parse_markdown(text: &str, fallback_title: &str) -> Vec<ImportedConversation> {
    let title_re = Regex::new(r"(?m)^#\s+(.+)$").unwrap();
    let title = title_re.captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| fallback_title.to_string());

    let role_re = Regex::new(r"(?im)^##\s*(User|Assistant|System|Developer|用户|助手|系统)\s*$").unwrap();
    let mut matches: Vec<(usize, usize, String)> = Vec::new();
    for mat in role_re.find_iter(text) {
        let full_match = mat.as_str();
        let role_word = full_match.trim_start_matches('#').trim();
        matches.push((mat.start(), mat.end(), role_word.to_string()));
    }

    let mut messages = Vec::new();
    let mut warnings = Vec::new();

    if matches.is_empty() {
        let body = title_re.replace(text, "").trim().to_string();
        messages.push(ImportedMessage {
            role: "user".to_string(),
            content: body,
            external_id: "markdown:1".to_string(),
            parent_external_id: None,
            created_at: None,
        });
        warnings.push("未发现显式角色标题，全文按一条用户消息导入".to_string());
    } else {
        let mut parent: Option<String> = None;
        for (i, (_start, end, role_word)) in matches.iter().enumerate() {
            let content_end = if i + 1 < matches.len() {
                matches[i + 1].0
            } else {
                text.len()
            };
            let content = text[*end..content_end].trim().to_string();
            let role = normalize_role(role_word);
            let ext_id = format!("markdown:{}", i + 1);

            messages.push(ImportedMessage {
                role,
                content,
                external_id: ext_id.clone(),
                parent_external_id: parent,
                created_at: None,
            });
            parent = Some(ext_id);
        }
    }

    let active_ext_id = messages.last().map(|m| m.external_id.clone());

    vec![ImportedConversation {
        title,
        external_id: format!("file-{:x}", Sha256::digest(text.as_bytes())),
        messages,
        active_external_id: active_ext_id,
        warnings,
    }]
}

pub fn parse_openai_messages(path: &Path) -> Result<Vec<ImportedConversation>, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("messages");
    let file_identity = format!("file-{:x}", Sha256::digest(text.as_bytes()));

    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
        if let Some(map) = val.as_object() {
            if let Some(_msgs) = map.get("messages").and_then(|m| m.as_array()) {
                let conv_id = map.get("conversation_id").or_else(|| map.get("id"))
                    .and_then(|s| s.as_str()).unwrap_or(&file_identity);
                return Ok(vec![linear_conversation(&val, conv_id)]);
            }
        }
        if let Some(arr) = val.as_array() {
            if let Some(first) = arr.first().and_then(|v| v.as_object()) {
                if first.contains_key("mapping") {
                    return Ok(arr.iter().enumerate().filter_map(|(idx, v)| chatgpt_conversation(v, idx)).collect());
                }
            }
            let mut list = Vec::new();
            for (idx, v) in arr.iter().enumerate() {
                if let Some(obj) = v.as_object() {
                    if obj.contains_key("messages") {
                        let fallback_id = format!("{}-{}", file_identity, idx);
                        let id_str = obj.get("id").or_else(|| obj.get("conversation_id"))
                            .and_then(|i| i.as_str()).unwrap_or(&fallback_id);
                        list.push(linear_conversation(v, id_str));
                    }
                }
            }
            if !list.is_empty() {
                return Ok(list);
            }
        }
    }

    // Attempt line by line JSONL
    let mut rows = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row_val: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("JSONL 第 {} 行无效：{}", line_idx + 1, e))?;
        rows.push(row_val);
    }

    if !rows.is_empty() {
        return Ok(parse_jsonl_rows(&rows, stem, &file_identity));
    }

    Err("JSON 顶层结构不是受支持的 conversations/messages 格式".to_string())
}

fn parse_jsonl_rows(rows: &[serde_json::Value], fallback_title: &str, file_identity: &str) -> Vec<ImportedConversation> {
    let mut groups: std::collections::HashMap<String, Vec<serde_json::Value>> = std::collections::HashMap::new();
    for r in rows {
        let conv_id = r.get("conversation_id").and_then(|c| c.as_str()).unwrap_or(file_identity).to_string();
        groups.entry(conv_id).or_default().push(r.clone());
    }

    let mut result = Vec::new();
    for (conv_id, items) in groups {
        let title = items.first()
            .and_then(|item| item.get("title").and_then(|t| t.as_str()))
            .map(|t| t.to_string())
            .unwrap_or_else(|| format!("{} · {}", fallback_title, conv_id));

        let payload = serde_json::json!({
            "title": title,
            "messages": items
        });
        result.push(linear_conversation(&payload, &conv_id));
    }
    result
}

fn linear_conversation(payload: &serde_json::Value, external_id: &str) -> ImportedConversation {
    let mut messages = Vec::new();
    let mut warnings = Vec::new();
    let mut parent: Option<String> = None;

    if let Some(arr) = payload.get("messages").and_then(|m| m.as_array()) {
        for (idx, item) in arr.iter().enumerate() {
            let role_val = item.get("role").and_then(|r| r.as_str()).unwrap_or("");
            // OpenAI-style content is either a string or a list of `{type, text}` parts.
            let content_owned = match item.get("content") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Array(parts)) => parts
                    .iter()
                    .filter_map(|p| p.as_str().or_else(|| p.get("text").and_then(|t| t.as_str())))
                    .collect::<Vec<_>>()
                    .join("\n"),
                _ => String::new(),
            };
            let content_val = content_owned.as_str();
            if role_val.is_empty() || content_val.trim().is_empty() {
                warnings.push(format!("跳过第 {} 条：role/content 无效", idx + 1));
                continue;
            }

            let msg_ext_id = item.get("id")
                .or_else(|| item.get("message_id"))
                .and_then(|i| i.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{}", idx + 1));

            let created_at = item.get("created_at").and_then(|c| c.as_str()).map(|s| s.to_string());

            messages.push(ImportedMessage {
                role: normalize_role(role_val),
                content: content_val.to_string(),
                external_id: msg_ext_id.clone(),
                parent_external_id: parent,
                created_at,
            });
            parent = Some(msg_ext_id);
        }
    }

    let title = payload.get("title").and_then(|t| t.as_str()).unwrap_or("导入会话").to_string();

    ImportedConversation {
        title,
        external_id: external_id.to_string(),
        messages,
        active_external_id: parent,
        warnings,
    }
}

pub fn parse_chatgpt(path: &Path) -> Result<Vec<ImportedConversation>, String> {
    if let Ok(file) = File::open(path) {
        if let Ok(mut archive) = ZipArchive::new(file) {
            // Exports also contain files such as shared_conversations.json, which list
            // conversations without messages; prefer the exact conversations.json.
            let mut names: Vec<String> = archive
                .file_names()
                .filter(|n| n.to_lowercase().ends_with(".json") && n.to_lowercase().contains("conversation"))
                .map(str::to_string)
                .collect();
            names.sort_by_key(|n| {
                let base = n.rsplit('/').next().unwrap_or(n).to_lowercase();
                (base != "conversations.json", n.len())
            });
            for name in names {
                let mut text = String::new();
                archive
                    .by_name(&name)
                    .map_err(|e| e.to_string())?
                    .read_to_string(&mut text)
                    .map_err(|e| format!("读取 {} 失败：{}", name, e))?;
                let val: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if let Some(arr) = val.as_array() {
                    let conversations: Vec<_> = arr
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, v)| chatgpt_conversation(v, idx))
                        .collect();
                    if !conversations.is_empty() {
                        return Ok(conversations);
                    }
                }
            }
            return Err("ZIP 中未找到包含会话的 conversations.json".to_string());
        }
    }

    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let val: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if let Some(arr) = val.as_array() {
        Ok(arr.iter().enumerate().filter_map(|(idx, v)| chatgpt_conversation(v, idx)).collect())
    } else if val.is_object() {
        chatgpt_conversation(&val, 0).map(|c| vec![c]).ok_or_else(|| "无法解析 ChatGPT 会话".to_string())
    } else {
        Err("ChatGPT 导出中没有有效会话".to_string())
    }
}

fn chatgpt_conversation(payload: &serde_json::Value, index: usize) -> Option<ImportedConversation> {
    let mapping = payload.get("mapping").and_then(|m| m.as_object())?;
    let mut warnings = Vec::new();

    // Tool calls, hidden system nodes and empty roots are skipped. Their children are
    // re-attached to the nearest kept ancestor so the conversation path stays intact.
    let mut kept: HashMap<&str, ImportedMessage> = HashMap::new();
    for (node_id, node) in mapping {
        let msg = match node.get("message") {
            Some(m) if m.is_object() => m,
            _ => continue,
        };

        let author_role = msg.get("author").and_then(|a| a.get("role")).and_then(|r| r.as_str()).unwrap_or("");
        if !matches!(author_role, "user" | "assistant" | "system") {
            continue;
        }
        let hidden = msg
            .get("metadata")
            .and_then(|m| m.get("is_visually_hidden_from_conversation"))
            .and_then(|h| h.as_bool())
            .unwrap_or(false);
        if hidden {
            continue;
        }

        let content = extract_chatgpt_content(msg.get("content"));
        if content.trim().is_empty() {
            continue;
        }

        let created_at = msg
            .get("create_time")
            .and_then(|t| t.as_f64())
            .and_then(crate::util::epoch_to_rfc3339);

        kept.insert(
            node_id.as_str(),
            ImportedMessage {
                role: normalize_role(author_role),
                content,
                external_id: node_id.clone(),
                parent_external_id: None,
                created_at,
            },
        );
    }

    let nearest_kept = |start: Option<&str>| -> Option<String> {
        let mut current = start;
        let mut steps = 0;
        while let Some(id) = current {
            if kept.contains_key(id) {
                return Some(id.to_string());
            }
            steps += 1;
            if steps > mapping.len() {
                return None;
            }
            current = mapping.get(id)?.get("parent").and_then(|p| p.as_str());
        }
        None
    };

    let parents: Vec<(&str, Option<String>)> = kept
        .keys()
        .map(|id| {
            let parent = mapping
                .get(*id)
                .and_then(|n| n.get("parent"))
                .and_then(|p| p.as_str());
            (*id, nearest_kept(parent))
        })
        .collect();
    for (id, parent) in parents {
        if let Some(message) = kept.get_mut(id) {
            message.parent_external_id = parent;
        }
    }

    let mut messages: Vec<ImportedMessage> = kept.into_values().collect();
    messages.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    let ext_id = payload.get("id")
        .or_else(|| payload.get("conversation_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}", index));

    let title = payload
        .get("title")
        .and_then(|t| t.as_str())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or("ChatGPT 导入会话")
        .to_string();
    let kept_ids: HashSet<&str> = messages.iter().map(|m| m.external_id.as_str()).collect();
    let mut current_node = payload.get("current_node").and_then(|c| c.as_str());
    let mut steps = 0;
    while let Some(id) = current_node {
        if kept_ids.contains(id) || steps > mapping.len() {
            break;
        }
        steps += 1;
        current_node = mapping.get(id).and_then(|n| n.get("parent")).and_then(|p| p.as_str());
    }
    let current_node = current_node
        .filter(|id| kept_ids.contains(id))
        .map(str::to_string);
    if messages.is_empty() {
        warnings.push(format!("“{}”没有可导入的文本消息", title));
    }

    Some(ImportedConversation {
        title,
        external_id: ext_id,
        messages,
        active_external_id: current_node,
        warnings,
    })
}

fn extract_chatgpt_content(content: Option<&serde_json::Value>) -> String {
    if let Some(c) = content {
        if let Some(parts) = c.get("parts").and_then(|p| p.as_array()) {
            let mut res = Vec::new();
            for p in parts {
                if let Some(s) = p.as_str() {
                    res.push(s);
                } else if let Some(s) = p.get("text").and_then(|t| t.as_str()) {
                    res.push(s);
                }
            }
            return res.join("\n");
        }
        if let Some(text) = c.get("text").and_then(|t| t.as_str()) {
            return text.to_string();
        }
    }
    String::new()
}

fn normalize_role(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "assistant" | "助手" => "assistant".to_string(),
        "system" | "系统" => "system".to_string(),
        "developer" => "developer".to_string(),
        _ => "user".to_string(),
    }
}
