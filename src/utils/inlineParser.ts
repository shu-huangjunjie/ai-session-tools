export type InlineToken =
  | { kind: 'text'; text: string }
  | { kind: 'bold'; text: string }
  | {
      kind: 'code';
      text: string;
      localFile?: { path: string; line?: number };
    }
  | {
      kind: 'file-link';
      label: string;
      path: string;
      line?: number;
      endLine?: number;
    }
  | {
      kind: 'conv-link';
      label: string;
      conversationId: string;
    }
  | {
      kind: 'web-link';
      label: string;
      url: string;
    };

export interface ParsedLocalTarget {
  path: string;
  line?: number;
  endLine?: number;
}

function safeDecodeUri(raw: string): string {
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

/**
 * Parses `file:///D:/path/to/file.py#L10-L20`, `file:///D:/path/file.py:10`,
 * or Windows drive paths `D:\path\to\file.py#L10` / `D:/path/to/file.py:10`.
 */
export function parseLocalFileTarget(rawHref: string): ParsedLocalTarget | null {
  const trimmed = rawHref.trim();
  if (!trimmed) return null;

  let body = trimmed;
  if (/^file:\/\//i.test(body)) {
    body = body.replace(/^file:\/\/\/?/i, '');
    // Ensure Windows drive letter doesn't keep a leading slash (`/D:/...` -> `D:/...`)
    body = body.replace(/^\/+([A-Za-z]:)/, '$1');
    body = safeDecodeUri(body);
  } else if (!/^[A-Za-z]:[\\/]/.test(body)) {
    return null;
  }

  let line: number | undefined;
  let endLine: number | undefined;

  // Match `#L12-L34` or `#L12`
  const hashMatch = body.match(/#L(\d+)(?:-L?(\d+))?$/i);
  if (hashMatch) {
    line = parseInt(hashMatch[1], 10);
    if (hashMatch[2]) endLine = parseInt(hashMatch[2], 10);
    body = body.slice(0, hashMatch.index);
  } else {
    // Match `:12:5` or `:12` after drive prefix (`D:\foo.py:12`)
    const colonMatch = body.match(/^([A-Za-z]:[\\/][^:*?"<>|]+?):(\d+)(?::\d+)?$/);
    if (colonMatch) {
      body = colonMatch[1];
      line = parseInt(colonMatch[2], 10);
    }
  }

  const normalizedPath = body.replace(/\//g, '\\').trim();
  if (!/^[A-Za-z]:\\/.test(normalizedPath)) {
    return null;
  }

  return { path: normalizedPath, line, endLine };
}

function basenameOfPath(winPath: string): string {
  const clean = winPath.replace(/[\\/]+$/, '');
  const idx = Math.max(clean.lastIndexOf('\\'), clean.lastIndexOf('/'));
  return idx >= 0 ? clean.slice(idx + 1) : clean;
}

const INLINE_MASTER_REGEX =
  /(\*\*[^*\n]+?\*\*|`[^`\n]+`|\[[^\]\n]+\]\((?:[^()\s]+|\([^()\s]*\))+\)|file:\/\/\/[^\s<>)"'`]+|https?:\/\/[^\s<>)"'`]+)/g;

export function tokenizeInline(text: string): InlineToken[] {
  if (!text) return [];
  const tokens: InlineToken[] = [];
  let lastIndex = 0;

  for (const match of text.matchAll(INLINE_MASTER_REGEX)) {
    const start = match.index ?? 0;
    let raw = match[0];

    if (start > lastIndex) {
      tokens.push({ kind: 'text', text: text.slice(lastIndex, start) });
    }

    // Trim trailing sentence punctuation accidentally swallowed by bare URLs
    let trailingText = '';
    if (/^(?:file:\/\/\/|https?:\/\/)/i.test(raw)) {
      const punct = raw.match(/[.,;!?，。；！？、）\]]+$/);
      if (punct) {
        trailingText = punct[0];
        raw = raw.slice(0, -trailingText.length);
      }
    }

    if (raw.length > 4 && raw.startsWith('**') && raw.endsWith('**')) {
      tokens.push({ kind: 'bold', text: raw.slice(2, -2) });
    } else if (raw.length > 2 && raw.startsWith('`') && raw.endsWith('`')) {
      const inner = raw.slice(1, -1);
      const parsed = parseLocalFileTarget(inner);
      tokens.push({
        kind: 'code',
        text: inner,
        localFile: parsed ? { path: parsed.path, line: parsed.line } : undefined,
      });
    } else if (raw.startsWith('[')) {
      const mdMatch = raw.match(/^\[([^\]\n]+)\]\(([\s\S]+)\)$/);
      if (mdMatch) {
        const label = mdMatch[1].trim();
        const href = mdMatch[2].trim();
        const convMatch = href.match(/^conversation:\/\/([A-Za-z0-9_-]+)/i);
        if (convMatch) {
          tokens.push({
            kind: 'conv-link',
            label,
            conversationId: convMatch[1],
          });
        } else {
          const local = parseLocalFileTarget(href);
          if (local) {
            tokens.push({
              kind: 'file-link',
              label: label || basenameOfPath(local.path),
              path: local.path,
              line: local.line,
              endLine: local.endLine,
            });
          } else if (/^https?:\/\//i.test(href)) {
            tokens.push({
              kind: 'web-link',
              label: label || href,
              url: href,
            });
          } else {
            tokens.push({ kind: 'text', text: raw });
          }
        }
      } else {
        tokens.push({ kind: 'text', text: raw });
      }
    } else if (/^file:\/\/\//i.test(raw)) {
      const local = parseLocalFileTarget(raw);
      if (local) {
        tokens.push({
          kind: 'file-link',
          label: basenameOfPath(local.path),
          path: local.path,
          line: local.line,
          endLine: local.endLine,
        });
      } else {
        tokens.push({ kind: 'text', text: raw });
      }
    } else if (/^https?:\/\//i.test(raw)) {
      tokens.push({
        kind: 'web-link',
        label: raw,
        url: raw,
      });
    } else {
      tokens.push({ kind: 'text', text: raw });
    }

    if (trailingText) {
      tokens.push({ kind: 'text', text: trailingText });
    }

    lastIndex = start + match[0].length;
  }

  if (lastIndex < text.length) {
    tokens.push({ kind: 'text', text: text.slice(lastIndex) });
  }

  return tokens;
}

export function isRunnableCodeLang(lang: string): boolean {
  const norm = lang.trim().toLowerCase();
  return [
    'python',
    'py',
    'typescript',
    'ts',
    'javascript',
    'js',
    'powershell',
    'ps1',
    'pwsh',
    'batch',
    'bat',
    'cmd',
  ].includes(norm);
}
