export interface ImportPreview {
  source_type: string;
  source_path: string;
  source_sha256: string;
  conversation_count: number;
  message_count: number;
  warning_count: number;
  warnings: string[];
  sample_titles: string[];
}

export interface ImportResult {
  source_type: string;
  conversation_count: number;
  new_conversation_count: number;
  message_count: number;
  warning_count: number;
  warnings: string[];
}

export interface AppPathsInfo {
  root: string;
  data: string;
  database: string;
  backups: string;
  logs: string;
}
