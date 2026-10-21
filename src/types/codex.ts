export interface CodexProject {
  project_id: string;
  name: string;
  root_paths: string[];
}

/** Where the Codex sidebar lists a thread; `hidden` threads are only kept in its databases. */
export type CodexScope = 'project' | 'chat' | 'hidden';

export interface CodexPlacement {
  scope: CodexScope;
  /** The local project for `project`. */
  project_id?: string | null;
}

export interface CodexVisibility {
  /** False when Codex keeps no project state; the Codex tab then groups by folder. */
  available: boolean;
  /** The sidebar's projects, in sidebar order. */
  projects: CodexProject[];
  /** Placement by vault conversation ID; conversations left out are hidden in Codex. */
  conversations: Record<string, CodexPlacement>;
}

export interface CodexThreadCandidate {
  thread_id: string;
  title: string;
  rollout_path: string;
  project_path?: string;
  exists_locally: boolean;
  model?: string;
  observed_providers: string[];
  updated_at: string;
  archived: boolean;
  source_indexes: string[];
  /** The rollout file is gone; messages are read from Codex's history database. */
  history_only?: boolean;
  /** Absent when Codex keeps no project state. */
  codex_scope?: CodexScope | null;
  codex_project_id?: string | null;
}

export interface CodexRecoveryReport {
  candidates: CodexThreadCandidate[];
  index_count: number;
  missing_rollouts: number;
  excluded_count: number;
  warnings: string[];
  /** The Codex sidebar's projects in sidebar order; absent when Codex keeps no project state. */
  sidebar_projects?: CodexProject[] | null;
}

export interface CodexRecoveryResult {
  conversation_count: number;
  new_conversation_count: number;
  message_count: number;
  title_update_count: number;
  skipped_count: number;
  warnings: string[];
}

export interface CodexPublishPreview {
  active_provider: string;
  eligible_thread_count: number;
  selected_thread_count: number;
  hidden_thread_count: number;
  project_count: number;
  existing_catalog_count: number;
}

export interface CodexPublishResult {
  active_provider: string;
  state_thread_count: number;
  catalog_thread_count: number;
  hidden_thread_count: number;
  purged_thread_count: number;
  removed_catalog_count: number;
  backup_directory: string;
  quarantined_rollout_count: number;
  quarantine_warnings: string[];
}
