/** Each client keeps its own accounts; switching one never touches another. */
export type AccountClient = 'codex' | 'claude' | 'antigravity';

export type AccountViewMode = 'grid' | 'list' | 'compact';

export type AccountSortField = 'added' | 'last_used' | 'remaining' | 'reset' | 'name';

export type AccountSortDirection = 'asc' | 'desc';

export type AccountStatusFilter = 'all' | 'current' | 'available' | 'low' | 'error' | 'verification';

export interface QuotaWindow {
  /** Codex `primary`/`secondary`, Claude `five_hour`/`seven_day…`, Antigravity model id. */
  kind: string;
  /** Model name for per-model limits; otherwise named by window length. */
  label?: string | null;
  used_percent: number;
  window_minutes?: number | null;
  resets_at?: string | null;
}

export interface Quota {
  plan_type?: string | null;
  windows: QuotaWindow[];
  /** Extra credits or overage, already formatted; `unlimited` for unlimited plans. */
  credits_balance?: string | null;
}

/** A saved account. Tokens stay in the backend and never reach the UI. */
export interface AccountView {
  id: string;
  /** `oauth` (official login) or `provider` (API relay/provider). */
  kind?: 'oauth' | 'provider' | null;
  email?: string | null;
  name?: string | null;
  account_id?: string | null;
  plan_type?: string | null;
  /** e.g. a relay's URL and model. */
  detail?: string | null;
  is_current: boolean;
  access_expires_at?: string | null;
  last_refresh?: string | null;
  added_at: string;
  last_used_at?: string | null;
  /** Relays have no common quota API. */
  has_quota: boolean;
  quota?: Quota | null;
  quota_error?: string | null;
  quota_updated_at?: string | null;
  verification_url?: string | null;
  tags?: string[];
  note?: string | null;
  base_url?: string | null;
  model?: string | null;
  auth_field?: string | null;
  haiku_model?: string | null;
  sonnet_model?: string | null;
  opus_model?: string | null;
}

export interface CurrentLogin {
  kind?: 'oauth' | 'provider' | null;
  email?: string | null;
  name?: string | null;
  plan_type?: string | null;
  /** The current login is already saved as an account. */
  saved: boolean;
}

export interface AccountsState {
  auth_path: string;
  current?: CurrentLogin | null;
  /** Why switching is not possible right now. */
  current_note?: string | null;
  notes: string[];
  accounts: AccountView[];
  cockpit_import_count?: number;
  cc_switch_available?: boolean;
}

export interface QuotaRefreshFailure {
  id: string;
  error: string;
}

export interface TokenImportResult {
  imported: number;
  failures: string[];
}

export interface CodexDeviceAuthStart {
  device_auth_id: string;
  user_code: string;
  verification_url: string;
  interval_secs: number;
}

export interface ProviderTestResult {
  ok: boolean;
  latency_ms: number;
  models: string[];
  message: string;
}

/** Adds or edits a Claude relay; an empty token keeps the saved one when editing. */
export interface ClaudeProviderInput {
  id?: string | null;
  name: string;
  base_url: string;
  token?: string | null;
  model?: string | null;
  auth_field?: string | null;
  haiku_model?: string | null;
  sonnet_model?: string | null;
  opus_model?: string | null;
}

/** Adds or edits a Codex API key provider; an empty api_key keeps the saved one when editing. */
export interface CodexProviderInput {
  id?: string | null;
  name: string;
  api_key: string;
  base_url?: string | null;
  model?: string | null;
}
