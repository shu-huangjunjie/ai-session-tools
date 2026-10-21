import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Check, Copy, User, Bot, Terminal, Shield, Brain, ChevronRight } from 'lucide-react';
import { CanonicalMessage, MessageRole } from '../types';
import { formatDateTime } from '../utils/formatters';
import { useI18n } from '../i18n';

interface MessageBubbleProps {
  message: CanonicalMessage;
}

type Segment =
  | { kind: 'markdown'; text: string }
  | { kind: 'details'; summary: string; body: string };

/** Claude thinking blocks are stored as `<details><summary>…</summary>…</details>`. */
export function splitDetails(content: string): Segment[] {
  const segments: Segment[] = [];
  const pattern = /<details>\s*<summary>([\s\S]*?)<\/summary>([\s\S]*?)<\/details>/g;
  let last = 0;
  for (const match of content.matchAll(pattern)) {
    const start = match.index ?? 0;
    if (start > last) segments.push({ kind: 'markdown', text: content.slice(last, start) });
    segments.push({
      kind: 'details',
      summary: match[1].replace(/<[^>]+>/g, '').trim(),
      body: match[2].trim(),
    });
    last = start + match[0].length;
  }
  if (last < content.length) segments.push({ kind: 'markdown', text: content.slice(last) });
  return segments;
}

async function writeClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

const roleMeta: Record<
  MessageRole,
  { label: string; icon: React.ReactNode; badgeClass: string; cardClass: string }
> = {
  user: {
    label: 'User',
    icon: <User size={12} className="text-[#1d1d1f] dark:text-[#f5f5f7]" />,
    badgeClass:
      'bg-black/[0.05] dark:bg-white/[0.08] text-[#1d1d1f] dark:text-[#f5f5f7] border-black/[0.08] dark:border-white/[0.12]',
    cardClass:
      'bg-white dark:bg-[#1c1c1e]/90 border-black/[0.08] dark:border-white/[0.1] text-[#1d1d1f] dark:text-[#f5f5f7]',
  },
  assistant: {
    label: 'Assistant',
    icon: <Bot size={12} className="text-[#0071e3] dark:text-[#2997ff]" />,
    badgeClass:
      'bg-[#0071e3]/10 dark:bg-[#0071e3]/15 text-[#0071e3] dark:text-[#2997ff] border-[#0071e3]/20 dark:border-[#0071e3]/30',
    cardClass:
      'bg-white/80 dark:bg-[#121214]/80 border-black/[0.06] dark:border-white/[0.07] text-[#1d1d1f] dark:text-[#ededf0]',
  },
  system: {
    label: 'System',
    icon: <Terminal size={12} className="text-[#af52de] dark:text-[#bf5af2]" />,
    badgeClass:
      'bg-[#af52de]/10 dark:bg-[#bf5af2]/15 text-[#af52de] dark:text-[#bf5af2] border-[#af52de]/20 dark:border-[#bf5af2]/30',
    cardClass:
      'bg-[#fdfbfd] dark:bg-[#141218]/80 border-black/[0.06] dark:border-white/[0.07] text-[#1d1d1f] dark:text-[#ededf0]',
  },
  developer: {
    label: 'Developer',
    icon: <Shield size={12} className="text-[#ff9500] dark:text-[#ff9f0a]" />,
    badgeClass:
      'bg-[#ff9500]/10 dark:bg-[#ff9f0a]/15 text-[#ff9500] dark:text-[#ff9f0a] border-[#ff9500]/20 dark:border-[#ff9f0a]/30',
    cardClass:
      'bg-[#fffdfa] dark:bg-[#181612]/80 border-black/[0.06] dark:border-white/[0.07] text-[#1d1d1f] dark:text-[#ededf0]',
  },
};

// Render text with bold and inline code
const renderInlineFormatted = (text: string): React.ReactNode[] => {
  const tokens = text.split(/(\*\*.+?\*\*|`[^`]+`)/g);
  return tokens.map((token, idx) => {
    if (token.length > 4 && token.startsWith('**') && token.endsWith('**')) {
      return (
        <strong key={idx} className="font-semibold text-black dark:text-white">
          {token.slice(2, -2)}
        </strong>
      );
    }
    if (token.length > 2 && token.startsWith('`') && token.endsWith('`')) {
      return (
        <code
          key={idx}
          className="bg-black/[0.05] dark:bg-white/[0.08] text-[#0071e3] dark:text-[#2997ff] px-1.5 py-0.5 rounded-md font-mono text-[11px] border border-black/[0.06] dark:border-white/[0.06]"
        >
          {token.slice(1, -1)}
        </code>
      );
    }
    return token;
  });
};

interface CodeBlockProps {
  lang: string;
  code: string;
  copyLabel: string;
  copiedLabel: string;
}

const CodeBlock: React.FC<CodeBlockProps> = ({ lang, code, copyLabel, copiedLabel }) => {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = async () => {
    if (await writeClipboard(code)) {
      setCopied(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 2000);
    }
  };

  return (
    <div className="my-3 rounded-2xl overflow-hidden border border-black/[0.08] dark:border-white/[0.08] bg-[#fbfbfd] dark:bg-[#0c0c0e] shadow-md dark:shadow-lg">
      <div className="flex items-center justify-between px-3.5 py-2 bg-black/[0.02] dark:bg-white/[0.03] border-b border-black/[0.06] dark:border-white/[0.06] select-none">
        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 rounded-full bg-[#ff5f56]/80 inline-block" />
            <span className="w-2.5 h-2.5 rounded-full bg-[#ffbd2e]/80 inline-block" />
            <span className="w-2.5 h-2.5 rounded-full bg-[#27c93f]/80 inline-block" />
          </div>
          {lang && (
            <span className="text-[11px] font-mono text-[#86868b] ml-1 uppercase tracking-wider font-medium">
              {lang}
            </span>
          )}
        </div>
        <button
          onClick={copy}
          title={copyLabel}
          className="flex items-center gap-1 text-[11px] text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white px-2 py-0.5 rounded-full hover:bg-black/[0.05] dark:hover:bg-white/[0.08] transition-all"
        >
          {copied ? <Check size={11} className="text-[#30d158]" /> : <Copy size={11} />}
          <span>{copied ? copiedLabel : copyLabel}</span>
        </button>
      </div>
      <pre className="p-4 overflow-x-auto text-[12px] font-mono text-[#1d1d1f] dark:text-[#e5e5ea] leading-relaxed select-text">
        <code>{code}</code>
      </pre>
    </div>
  );
};

interface Labels {
  copyCode: string;
  copiedCode: string;
  thinking: string;
}

/** Renders a markdown subset: fenced code, tables, headings, quotes and lists. */
function renderMarkdown(text: string, labels: Labels, keyPrefix: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const parts = text.split(/(```[^\n]*\n[\s\S]*?```)/g);

  parts.forEach((part, partIdx) => {
    const key = `${keyPrefix}-${partIdx}`;
    const fence = part.match(/^```([^\n]*)\n([\s\S]*?)\n?```$/);
    if (fence) {
      const lang = fence[1].trim().split(/\s+/)[0] ?? '';
      nodes.push(
        <CodeBlock
          key={key}
          lang={lang}
          code={fence[2]}
          copyLabel={labels.copyCode}
          copiedLabel={labels.copiedCode}
        />
      );
      return;
    }

    part
      .split(/\n{2,}/g)
      .filter((block) => block.trim() !== '')
      .forEach((block, blockIdx) => nodes.push(renderBlock(block, `${key}-${blockIdx}`)));
  });
  return nodes;
}

function renderBlock(block: string, key: string): React.ReactNode {
  const lines = block.replace(/^\n+|\n+$/g, '').split('\n');

  const isTable =
    lines.length >= 2 &&
    lines[0].trim().startsWith('|') &&
    lines[0].trim().endsWith('|') &&
    /^\s*\|?[\s:|-]+\|?\s*$/.test(lines[1]) &&
    lines[1].includes('-');

  if (isTable) {
    const cells = (row: string) =>
      row
        .trim()
        .replace(/^\||\|$/g, '')
        .split('|')
        .map((c) => c.trim());
    const headerCols = cells(lines[0]);
    const bodyRows = lines.slice(2).filter((l) => l.trim().startsWith('|'));

    return (
      <div key={key} className="my-3 overflow-x-auto">
        <table className="min-w-full text-xs rounded-xl overflow-hidden border border-black/[0.08] dark:border-white/[0.08] bg-white dark:bg-white/[0.02]">
          <thead>
            <tr className="bg-black/[0.03] dark:bg-white/[0.06] border-b border-black/[0.08] dark:border-white/[0.08]">
              {headerCols.map((h, i) => (
                <th key={i} className="px-3.5 py-2 text-left font-semibold text-[#1d1d1f] dark:text-[#f5f5f7]">
                  {renderInlineFormatted(h)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-black/[0.04] dark:divide-white/[0.04]">
            {bodyRows.map((row, rIdx) => (
              <tr key={rIdx} className="hover:bg-black/[0.02] dark:hover:bg-white/[0.03] transition-colors">
                {cells(row).map((col, cIdx) => (
                  <td key={cIdx} className="px-3.5 py-2 text-[#333336] dark:text-[#d2d2d7]">
                    {renderInlineFormatted(col)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  if (lines.every((l) => l.trimStart().startsWith('>'))) {
    return (
      <blockquote
        key={key}
        className="my-2 border-l-[3px] border-[#0071e3] pl-3.5 py-1.5 rounded-r-lg bg-black/[0.03] dark:bg-white/[0.03] text-[#6e6e73] dark:text-[#a1a1a6]"
      >
        {lines.map((l, i) => (
          <div key={i}>{renderInlineFormatted(l.trimStart().replace(/^>\s?/, ''))}</div>
        ))}
      </blockquote>
    );
  }

  return (
    <div key={key} className="space-y-1 my-1.5 leading-relaxed text-[#333336] dark:text-[#d2d2d7]">
      {lines.map((line, lIdx) => {
        const trimmedLine = line.trim();
        const heading = trimmedLine.match(/^(#{1,4})\s+(.*)$/);
        if (heading) {
          const size = ['text-[15px]', 'text-sm', 'text-[13px]', 'text-xs'][heading[1].length - 1];
          return (
            <div
              key={lIdx}
              className={`${size} font-semibold text-[#1d1d1f] dark:text-[#f5f5f7] mt-2 mb-0.5 tracking-tight`}
            >
              {renderInlineFormatted(heading[2])}
            </div>
          );
        }
        if (/^[-*+]\s+/.test(trimmedLine)) {
          const indent = Math.min(3, Math.floor((line.length - line.trimStart().length) / 2));
          return (
            <div key={lIdx} className="flex items-start gap-2" style={{ paddingLeft: 8 + indent * 14 }}>
              <span className="text-[#86868b] select-none mt-1 text-[8px]">•</span>
              <span className="flex-1">{renderInlineFormatted(trimmedLine.replace(/^[-*+]\s+/, ''))}</span>
            </div>
          );
        }
        const orderedMatch = trimmedLine.match(/^(\d+)[.)]\s+(.*)$/);
        if (orderedMatch) {
          return (
            <div key={lIdx} className="flex items-start gap-2 pl-2">
              <span className="text-[#86868b] select-none font-mono text-[11px] mt-0.5">{orderedMatch[1]}.</span>
              <span className="flex-1">{renderInlineFormatted(orderedMatch[2])}</span>
            </div>
          );
        }
        if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmedLine)) {
          return <hr key={lIdx} className="my-2 border-black/[0.08] dark:border-white/[0.08]" />;
        }
        if (trimmedLine === '') {
          return <div key={lIdx} className="h-1.5" />;
        }
        return (
          <div key={lIdx} className="whitespace-pre-wrap break-words">
            {renderInlineFormatted(line)}
          </div>
        );
      })}
    </div>
  );
}

const ThinkingBlock: React.FC<{ summary: string; body: string; labels: Labels; id: string }> = ({
  summary,
  body,
  labels,
  id,
}) => (
  <details className="group/think my-2.5 rounded-2xl border border-[#af52de]/20 bg-[#af52de]/[0.04] dark:bg-[#bf5af2]/[0.06] overflow-hidden">
    <summary className="flex items-center gap-1.5 px-3.5 py-2 cursor-pointer select-none text-[11px] font-medium text-[#af52de] dark:text-[#bf5af2] list-none [&::-webkit-details-marker]:hidden">
      <ChevronRight size={12} className="transition-transform group-open/think:rotate-90" />
      <Brain size={12} />
      <span>{summary.replace(/^💭\s*/u, '') || labels.thinking}</span>
    </summary>
    <div className="px-3.5 pb-3 pt-1 text-[#6e6e73] dark:text-[#a1a1a6] border-t border-[#af52de]/10">
      {renderMarkdown(body, labels, id)}
    </div>
  </details>
);

export const MessageBubble: React.FC<MessageBubbleProps> = React.memo(({ message }) => {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => clearTimeout(timer.current), []);

  const labels = useMemo<Labels>(
    () => ({ copyCode: t.chat.copyCode, copiedCode: t.chat.copiedCode, thinking: t.chat.thinking }),
    [t]
  );

  const body = useMemo(
    () =>
      splitDetails(message.content).map((segment, idx) =>
        segment.kind === 'details' ? (
          <ThinkingBlock
            key={`d-${idx}`}
            summary={segment.summary}
            body={segment.body}
            labels={labels}
            id={`d-${idx}`}
          />
        ) : (
          <React.Fragment key={`m-${idx}`}>{renderMarkdown(segment.text, labels, `m-${idx}`)}</React.Fragment>
        )
      ),
    [message.content, labels]
  );

  const handleCopy = useCallback(async () => {
    if (await writeClipboard(message.content)) {
      setCopied(true);
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 2000);
    }
  }, [message.content]);

  const meta = roleMeta[message.role] || roleMeta.user;

  return (
    <div
      className={`p-4 md:p-5 rounded-3xl border ${meta.cardClass} shadow-[0_2px_16px_rgba(0,0,0,0.04)] dark:shadow-[0_4px_24px_rgba(0,0,0,0.35)] group transition-colors duration-200 hover:border-black/[0.12] dark:hover:border-white/[0.15]`}
    >
      <div className="flex items-center justify-between pb-3 mb-2.5 border-b border-black/[0.06] dark:border-white/[0.06]">
        <div className="flex items-center gap-2.5 min-w-0">
          <div
            className={`px-2.5 py-0.5 rounded-full border text-[11px] font-medium flex items-center gap-1.5 ${meta.badgeClass}`}
          >
            {meta.icon}
            <span>{meta.label}</span>
          </div>

          {message.model && (
            <span className="text-[10px] text-[#86868b] bg-black/[0.04] dark:bg-white/[0.04] px-2 py-0.5 rounded-full border border-black/[0.06] dark:border-white/[0.06] font-mono truncate">
              {message.model}
            </span>
          )}

          {message.created_at && (
            <span className="text-[11px] text-[#86868b] tracking-tight">{formatDateTime(message.created_at)}</span>
          )}
        </div>

        <button
          onClick={handleCopy}
          title={t.chat.copyMessageTooltip}
          className="opacity-0 group-hover:opacity-100 focus:opacity-100 flex items-center gap-1.5 text-[11px] text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-[#f5f5f7] px-2.5 py-1 rounded-full bg-black/[0.04] dark:bg-white/[0.06] hover:bg-black/[0.08] dark:hover:bg-white/[0.1] border border-black/[0.06] dark:border-white/[0.08] transition-all"
        >
          {copied ? <Check size={11} className="text-[#30d158]" /> : <Copy size={11} />}
          <span>{copied ? t.chat.copiedCode : t.chat.copyMessage}</span>
        </button>
      </div>

      <div className="text-xs markdown-body select-text leading-relaxed">{body}</div>
    </div>
  );
});

MessageBubble.displayName = 'MessageBubble';
