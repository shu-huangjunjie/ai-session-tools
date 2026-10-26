/** Client whose MCP servers are managed: Claude Code (`~/.claude.json`), Codex (`config.toml`) or Antigravity (`~/.gemini/config/mcp_config.json`). */
export type McpClient = 'claude' | 'codex' | 'antigravity';

export type McpTransport = 'stdio' | 'http' | 'sse';

/**
 * A server in Claude's JSON form: `type`, `command`, `args`, `env` (stdio) or `url`, `headers`
 * (http / sse), plus any other fields the config file carries. Codex entries are converted to this
 * form; the backend converts back when writing `config.toml`.
 */
export type McpSpec = Record<string, unknown>;

export interface McpServerEntry {
  client: McpClient;
  name: string;
  transport: McpTransport | string;
  spec: McpSpec;
  /** Claude: false when moved to the app's stash; Codex: `enabled = false`; Antigravity: `"disabled": true`. */
  enabled: boolean;
  /** Written and maintained by the Codex desktop app. */
  read_only: boolean;
  /** Whether cc-switch enables a server of this name for this client; null when it has none. */
  cc_switch?: boolean | null;
  /** The entry as written in the config file. */
  raw: string;
}

export interface McpLocation {
  client: McpClient;
  config_path: string;
  exists: boolean;
  /** Set when the config file cannot be parsed; writes are then refused. */
  error?: string | null;
  servers: McpServerEntry[];
}

export interface McpCopyOutcome {
  /** Fields the target client does not support, left out of the copy. */
  dropped_fields: string[];
}
