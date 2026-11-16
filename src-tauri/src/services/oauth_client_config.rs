//! Optional local Google OAuth client parameters. Values are never logged.
use serde::Deserialize;
use std::path::PathBuf;

const FILE_NAME: &str = "oauth-clients.local.json";

#[derive(Deserialize)]
struct Client {
    client_id: String,
    client_secret: String,
}

fn pair(id: String, secret: String) -> Option<(String, String)> {
    let id = id.trim();
    let secret = secret.trim();
    (!id.is_empty() && !secret.is_empty()).then(|| (id.to_string(), secret.to_string()))
}

fn from_paths(paths: &[PathBuf]) -> Option<(String, String)> {
    paths.iter().find_map(|path| {
        let content = std::fs::read_to_string(path).ok()?;
        let client: Client = serde_json::from_str(&content).ok()?;
        pair(client.client_id, client.client_secret)
    })
}

pub(super) fn configured_client() -> Option<(String, String)> {
    if let (Ok(id), Ok(secret)) = (
        std::env::var("ANTIGRAVITY_GOOGLE_CLIENT_ID"),
        std::env::var("ANTIGRAVITY_GOOGLE_CLIENT_SECRET"),
    ) {
        if let Some(client) = pair(id, secret) {
            return Some(client);
        }
    }
    let mut paths = Vec::new();
    if let Some(file) = std::env::var_os("CONTEXT_VAULT_OAUTH_CLIENT_FILE") {
        paths.push(PathBuf::from(file));
    }
    if let Some(root) = std::env::var_os("CONTEXT_VAULT_ROOT") {
        paths.push(PathBuf::from(root).join(FILE_NAME));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            paths.push(parent.join(FILE_NAME));
        }
    }
    if let Some(data) = dirs::data_local_dir() {
        paths.push(data.join("ContextRestoration").join(FILE_NAME));
    }
    #[cfg(debug_assertions)]
    if let Some(project) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
        paths.push(project.join(FILE_NAME));
    }
    from_paths(&paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_complete_local_parameters_without_requiring_an_installed_client() {
        let root = std::env::temp_dir().join(format!("cv-oauth-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let first = root.join("first.json");
        let fallback = root.join("fallback.json");
        std::fs::write(&first, r#"{"client_id":"incomplete"}"#).unwrap();
        std::fs::write(&fallback, r#"{"client_id":" local-client ","client_secret":" local-secret "}"#).unwrap();
        let paths = [first.clone(), fallback.clone()];
        assert_eq!(from_paths(&paths), Some(("local-client".into(), "local-secret".into())));
        std::fs::write(&first, r#"{"client_id":"preferred","client_secret":"preferred-secret"}"#).unwrap();
        assert_eq!(from_paths(&paths).unwrap().0, "preferred");
        std::fs::write(&first, "invalid json").unwrap();
        std::fs::write(&fallback, r#"{"client_id":"client","client_secret":"   "}"#).unwrap();
        assert_eq!(from_paths(&paths), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn never_combines_incomplete_or_blank_client_parameters() {
        assert_eq!(pair("id".into(), " ".into()), None);
        assert_eq!(pair(" ".into(), "secret".into()), None);
        assert_eq!(from_paths(&[]), None);
    }
}
