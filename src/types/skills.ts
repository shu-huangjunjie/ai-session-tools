/** Client that loads skills from its own skills folder. */
export type SkillClient = 'claude' | 'codex';

/** A client skills folder, or a library folder the user registered. */
export type SkillLocationKind = SkillClient | 'library';

export interface SkillEntry {
  /** Folder name; the name the client loads the skill under. */
  folder_name: string;
  /** `name` in the SKILL.md front matter. */
  name?: string | null;
  description?: string | null;
  path: string;
  /** Target of a directory junction or symlink; absent for a real folder. */
  link_target?: string | null;
  link_broken: boolean;
  has_skill_file: boolean;
  /** False when moved to `skills-disabled`, which the client does not load. */
  enabled: boolean;
  /** Codex built-in skill under `skills/.system`; read-only. */
  system: boolean;
  file_count: number;
  total_bytes: number;
  modified_at?: string | null;
  /** Clients a library skill is mounted into. */
  mounted_in: SkillClient[];
}

export interface SkillLocation {
  kind: SkillLocationKind;
  root: string;
  disabled_root?: string | null;
  exists: boolean;
  skills: SkillEntry[];
}

/** A link deletion leaves its target untouched; a real folder goes to the Recycle Bin. */
export type SkillDeleteOutcome = 'link_removed' | 'recycled';
