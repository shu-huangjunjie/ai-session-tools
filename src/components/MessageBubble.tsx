import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Check,
  Copy,
  User,
  Bot,
  Terminal,
  Shield,
  Brain,
  ChevronRight,
  FileCode2,
  ExternalLink,
  MessageSquareShare,
  Play,
  Sparkles,
} from 'lucide-react';
import { CanonicalMessage, MessageRole } from '../types';
import { formatDateTime } from '../utils/formatters';
import { isRunnableCodeLang, tokenizeInline } from '../utils/inlineParser';
import { useI18n } from '../i18n';

interface MessageBubbleProps {
  message: CanonicalMessage;
  onOpenLocalFile?: (path: string, line?: number) => void;
  onSendCodeToPrism?: (lang: string, code: string, autoRun: boolean) => void;
  onSelectConversation?: (conversationId: string) => void;
  onOpenExternalUrl?: (url: string) => void;
  highlightQuery?: string;
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
  {
    label: string;
    icon: React.ReactNode;
    badgeClass: string;
    cardClass: string;
    headerClass: string;
  }
> = {
  user: {
    label: 'User',
    icon: <User size={11.5} className="text-white" />,
    badgeClass:
      'bg-blue-600 dark:bg-blue-500 text-white border-blue-600 dark:border-blue-400/40 shadow-[0_1px_2px_rgba(37,99,235,0.2)]',
    cardClass:
      'bg-gradient-to-br from-[#eef4ff] via-[#f4f7ff] to-[#eef2ff]/90 dark:from-[#192742] dark:via-[#17233a] dark:to-[#162034] border-blue-200/85 dark:border-blue-400/25 text-slate-900 dark:text-slate-100 shadow-[0_2px_8px_rgba(37,99,235,0.05)] dark:shadow-[0_4px_18px_rgba(0,0,0,0.24)] hover:border-blue-300/90 dark:hover:border-blue-400/40',
    headerClass: 'mb-2',
  },
  assistant: {
    label: 'Assistant',
    icon: <Bot size={12} className="text-indigo-600 dark:text-indigo-300" />,
    badgeClass:
      'bg-indigo-50 dark:bg-indigo-500/15 text-indigo-600 dark:text-indigo-300 border-indigo-200/85 dark:border-indigo-400/25',
    cardClass:
      'bg-white dark:bg-[#151e2e] border-slate-200/90 dark:border-slate-700/75 text-slate-800 dark:text-slate-200 shadow-[0_2px_10px_rgba(15,23,42,0.035)] dark:shadow-[0_4px_20px_rgba(0,0,0,0.26)] hover:border-slate-300/90 dark:hover:border-slate-600/80',
    headerClass: 'pb-2.5 mb-3 border-b border-slate-100 dark:border-slate-800/85',
  },
  system: {
    label: 'System',
    icon: <Terminal size={12} className="text-purple-600 dark:text-purple-300" />,
    badgeClass:
      'bg-purple-50 dark:bg-purple-500/15 text-purple-600 dark:text-purple-300 border-purple-200/85 dark:border-purple-400/25',
    cardClass:
      'bg-[#fcfaff] dark:bg-[#181628] border-purple-200/60 dark:border-purple-400/20 text-slate-800 dark:text-slate-200 shadow-sm',
    headerClass: 'pb-2.5 mb-2.5 border-b border-purple-100 dark:border-purple-500/15',
  },
  developer: {
    label: 'Developer',
    icon: <Shield size={12} className="text-amber-600 dark:text-amber-300" />,
    badgeClass:
      'bg-amber-50 dark:bg-amber-500/15 text-amber-700 dark:text-amber-300 border-amber-200/85 dark:border-amber-400/25',
    cardClass:
      'bg-[#fffdf8] dark:bg-[#1e1b16] border-amber-200/65 dark:border-amber-400/20 text-slate-800 dark:text-slate-200 shadow-sm',
    headerClass: 'pb-2.5 mb-2.5 border-b border-amber-100 dark:border-amber-500/15',
  },
};

interface InlineContext {
  onOpenLocalFile?: (path: string, line?: number) => void;
  onSelectConversation?: (conversationId: string) => void;
  onOpenExternalUrl?: (url: string) => void;
  openFileTooltipTemplate: string;
  jumpConvTooltipTemplate: string;
  highlightQuery?: string;
}

function highlightText(text: string, query?: string): React.ReactNode {
  const q = query?.trim();
  if (!q) return text;
  const escaped = q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const parts = text.split(new RegExp(`(${escaped})`, 'gi'));
  if (parts.length <= 1) return text;
  return parts.map((part, idx) =>
    part.toLowerCase() === q.toLowerCase() ? (
      <mark
        key={idx}
        className="bg-amber-300/85 dark:bg-amber-400/35 text-inherit rounded px-0.5"
      >
        {part}
      </mark>
    ) : (
      part
    )
  );
}

const renderInlineFormatted = (
  text: string,
  ctx: InlineContext
): React.ReactNode[] => {
  const tokens = tokenizeInline(text);
  return tokens.map((token, idx) => {
    switch (token.kind) {
      case 'bold':
        return (
          <strong key={idx} className="font-semibold text-slate-900 dark:text-white">
            {renderInlineFormatted(token.text, ctx)}
          </strong>
        );
      case 'code': {
        if (token.localFile && ctx.onOpenLocalFile) {
          const { path, line } = token.localFile;
          const tip = ctx.openFileTooltipTemplate.replace(
            '{path}',
            line ? `${path}:${line}` : path
          );
          return (
            <button
              key={idx}
              type="button"
              onClick={() => ctx.onOpenLocalFile?.(path, line)}
              title={tip}
              className="inline-flex items-center gap-1 bg-blue-50 dark:bg-blue-500/15 hover:bg-blue-100/90 dark:hover:bg-blue-500/25 text-blue-700 dark:text-sky-300 px-1.5 py-0.5 rounded-md font-mono text-[11.5px] border border-blue-200/85 dark:border-blue-400/30 transition-colors align-baseline"
            >
              <FileCode2 size={11} className="flex-shrink-0" />
              <span>{highlightText(token.text, ctx.highlightQuery)}</span>
            </button>
          );
        }
        return (
          <code
            key={idx}
            className="bg-slate-100 dark:bg-slate-800/90 text-blue-700 dark:text-sky-300 px-1.5 py-0.5 rounded-md font-mono text-[11.5px] border border-slate-200/80 dark:border-slate-700/75"
          >
            {highlightText(token.text, ctx.highlightQuery)}
          </code>
        );
      }
      case 'file-link': {
        const lineSuffix = token.line
          ? token.endLine && token.endLine !== token.line
            ? `#L${token.line}-${token.endLine}`
            : `#L${token.line}`
          : '';
        const tip = ctx.openFileTooltipTemplate.replace(
          '{path}',
          `${token.path}${lineSuffix}`
        );
        return (
          <button
            key={idx}
            type="button"
            onClick={() => ctx.onOpenLocalFile?.(token.path, token.line)}
            title={tip}
            className="inline-flex items-center gap-1 mx-0.5 px-1.5 py-0.5 rounded-md bg-indigo-50 dark:bg-indigo-500/15 hover:bg-indigo-100/90 dark:hover:bg-indigo-500/25 text-indigo-700 dark:text-indigo-300 border border-indigo-200/85 dark:border-indigo-400/30 font-medium text-[11.5px] transition-colors align-baseline"
          >
            <FileCode2 size={11} className="flex-shrink-0 opacity-85" />
            <span className="underline decoration-indigo-500/35 underline-offset-2">
              {highlightText(token.label, ctx.highlightQuery)}
            </span>
            {lineSuffix && (
              <span className="font-mono text-[10px] opacity-80">{lineSuffix}</span>
            )}
          </button>
        );
      }
      case 'conv-link': {
        const tip = ctx.jumpConvTooltipTemplate.replace('{id}', token.conversationId);
        return (
          <button
            key={idx}
            type="button"
            onClick={() => ctx.onSelectConversation?.(token.conversationId)}
            title={tip}
            className="inline-flex items-center gap-1 mx-0.5 px-1.5 py-0.5 rounded-md bg-purple-50 dark:bg-purple-500/15 hover:bg-purple-100/90 dark:hover:bg-purple-500/25 text-purple-700 dark:text-purple-300 border border-purple-200/85 dark:border-purple-400/25 text-[11.5px] font-medium transition-colors align-baseline"
          >
            <MessageSquareShare size={11} className="flex-shrink-0" />
            <span>{highlightText(token.label, ctx.highlightQuery)}</span>
          </button>
        );
      }
      case 'web-link':
        return (
          <button
            key={idx}
            type="button"
            onClick={() => ctx.onOpenExternalUrl?.(token.url)}
            title={token.url}
            className="inline-flex items-center gap-1 text-blue-600 dark:text-sky-400 hover:underline underline-offset-2 align-baseline break-all text-left"
          >
            <span>{highlightText(token.label, ctx.highlightQuery)}</span>
            <ExternalLink size={10} className="flex-shrink-0 opacity-75" />
          </button>
        );
      case 'text':
      default:
        return (
          <React.Fragment key={idx}>
            {highlightText(token.text, ctx.highlightQuery)}
          </React.Fragment>
        );
    }
  });
};

interface CodeBlockProps {
  lang: string;
  code: string;
  labels: Labels;
  onSendCodeToPrism?: (lang: string, code: string, autoRun: boolean) => void;
}

const CodeBlock: React.FC<CodeBlockProps> = ({
  lang,
  code,
  labels,
  onSendCodeToPrism,
}) => {
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

  const canRun = isRunnableCodeLang(lang);

  return (
    <div className="my-3 rounded-xl overflow-hidden border border-slate-200/95 dark:border-slate-700/85 bg-[#f8fafc] dark:bg-[#0d131f] shadow-[0_2px_8px_rgba(15,23,42,0.04)]">
      <div className="flex items-center justify-between gap-2 px-3.5 py-1.5 bg-slate-100/85 dark:bg-[#121a2a] border-b border-slate-200/85 dark:border-slate-800/90 select-none">
        <div className="flex items-center gap-1.5 min-w-0">
          <FileCode2 size={12} className="text-slate-400 dark:text-slate-500 flex-shrink-0" />
          <span className="text-[11px] font-mono text-slate-600 dark:text-slate-400 uppercase tracking-wider font-semibold truncate">
            {lang || 'code'}
          </span>
        </div>

        <div className="flex items-center gap-1 flex-shrink-0">
          {onSendCodeToPrism && canRun && (
            <button
              type="button"
              onClick={() => onSendCodeToPrism(lang || 'python', code, true)}
              title={labels.runCodeInPrismTooltip}
              className="flex items-center gap-1 text-[11px] text-emerald-700 dark:text-emerald-300 hover:bg-emerald-100/80 dark:hover:bg-emerald-500/20 bg-emerald-50 dark:bg-emerald-500/10 border border-emerald-200/85 dark:border-emerald-400/25 px-2 py-0.5 rounded-md font-medium transition-all"
            >
              <Play size={10} className="fill-current" />
              <span>{labels.runCodeInPrism}</span>
            </button>
          )}
          {onSendCodeToPrism && (
            <button
              type="button"
              onClick={() => onSendCodeToPrism(lang || 'python', code, false)}
              title={labels.sendCodeToPrismTooltip}
              className="flex items-center gap-1 text-[11px] text-indigo-600 dark:text-indigo-300 hover:bg-indigo-100/80 dark:hover:bg-indigo-500/20 bg-indigo-50 dark:bg-indigo-500/10 border border-indigo-200/85 dark:border-indigo-400/25 px-2 py-0.5 rounded-md font-medium transition-all"
            >
              <Sparkles size={10} />
              <span>{labels.sendCodeToPrism}</span>
            </button>
          )}
          <button
            type="button"
            onClick={copy}
            title={labels.copyCode}
            className="flex items-center gap-1 text-[11px] text-slate-500 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white px-2 py-0.5 rounded-md hover:bg-slate-200/70 dark:hover:bg-slate-800 transition-all"
          >
            {copied ? <Check size={11} className="text-emerald-600 dark:text-emerald-400" /> : <Copy size={11} />}
            <span>{copied ? labels.copiedCode : labels.copyCode}</span>
          </button>
        </div>
      </div>
      <pre className="p-3.5 overflow-x-auto text-[12px] font-mono text-slate-800 dark:text-slate-200 leading-relaxed select-text">
        <code>{code}</code>
      </pre>
    </div>
  );
};

interface Labels {
  copyCode: string;
  copiedCode: string;
  thinking: string;
  sendCodeToPrism: string;
  sendCodeToPrismTooltip: string;
  runCodeInPrism: string;
  runCodeInPrismTooltip: string;
}

/** Renders a markdown subset: fenced code, tables, headings, quotes and lists. */
function renderMarkdown(
  text: string,
  labels: Labels,
  inlineCtx: InlineContext,
  onSendCodeToPrism: ((lang: string, code: string, autoRun: boolean) => void) | undefined,
  keyPrefix: string
): React.ReactNode[] {
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
          labels={labels}
          onSendCodeToPrism={onSendCodeToPrism}
        />
      );
      return;
    }

    part
      .split(/\n{2,}/g)
      .filter((block) => block.trim() !== '')
      .forEach((block, blockIdx) =>
        nodes.push(renderBlock(block, inlineCtx, `${key}-${blockIdx}`))
      );
  });
  return nodes;
}

function renderBlock(
  block: string,
  inlineCtx: InlineContext,
  key: string
): React.ReactNode {
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
        <table className="min-w-full text-xs rounded-xl overflow-hidden border border-slate-200/90 dark:border-slate-700/80 bg-white dark:bg-[#111827]">
          <thead>
            <tr className="bg-slate-50 dark:bg-slate-800/80 border-b border-slate-200/85 dark:border-slate-700/80">
              {headerCols.map((h, i) => (
                <th
                  key={i}
                  className="px-3.5 py-2 text-left font-semibold text-slate-900 dark:text-slate-100"
                >
                  {renderInlineFormatted(h, inlineCtx)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-100 dark:divide-slate-800/80">
            {bodyRows.map((row, rIdx) => (
              <tr
                key={rIdx}
                className="hover:bg-slate-50/70 dark:hover:bg-white/[0.03] transition-colors"
              >
                {cells(row).map((col, cIdx) => (
                  <td key={cIdx} className="px-3.5 py-2 text-slate-700 dark:text-slate-300">
                    {renderInlineFormatted(col, inlineCtx)}
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
        className="my-2.5 border-l-[3px] border-blue-500 pl-3.5 py-1.5 rounded-r-lg bg-blue-50/50 dark:bg-blue-500/[0.08] text-slate-600 dark:text-slate-300"
      >
        {lines.map((l, i) => (
          <div key={i}>
            {renderInlineFormatted(l.trimStart().replace(/^>\s?/, ''), inlineCtx)}
          </div>
        ))}
      </blockquote>
    );
  }

  return (
    <div key={key} className="space-y-1.5 my-1.5 leading-[1.72] text-slate-700 dark:text-slate-200">
      {lines.map((line, lIdx) => {
        const trimmedLine = line.trim();
        const heading = trimmedLine.match(/^(#{1,4})\s+(.*)$/);
        if (heading) {
          const size = ['text-[15.5px]', 'text-[14.5px]', 'text-[13.5px]', 'text-[13px]'][
            heading[1].length - 1
          ];
          return (
            <div
              key={lIdx}
              className={`${size} font-semibold text-slate-900 dark:text-white mt-2.5 mb-1 tracking-tight`}
            >
              {renderInlineFormatted(heading[2], inlineCtx)}
            </div>
          );
        }
        if (/^[-*+]\s+/.test(trimmedLine)) {
          const indent = Math.min(
            3,
            Math.floor((line.length - line.trimStart().length) / 2)
          );
          return (
            <div
              key={lIdx}
              className="flex items-start gap-2"
              style={{ paddingLeft: 6 + indent * 14 }}
            >
              <span className="text-blue-500/80 dark:text-sky-400/80 select-none mt-1 text-[9px]">•</span>
              <span className="flex-1">
                {renderInlineFormatted(trimmedLine.replace(/^[-*+]\s+/, ''), inlineCtx)}
              </span>
            </div>
          );
        }
        const orderedMatch = trimmedLine.match(/^(\d+)[.)]\s+(.*)$/);
        if (orderedMatch) {
          return (
            <div key={lIdx} className="flex items-start gap-2 pl-1.5">
              <span className="text-slate-500 dark:text-slate-400 select-none font-mono font-semibold text-[11.5px] mt-0.5 min-w-[16px]">
                {orderedMatch[1]}.
              </span>
              <span className="flex-1">
                {renderInlineFormatted(orderedMatch[2], inlineCtx)}
              </span>
            </div>
          );
        }
        if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmedLine)) {
          return <hr key={lIdx} className="my-2.5 border-slate-200 dark:border-slate-800" />;
        }
        if (trimmedLine === '') {
          return <div key={lIdx} className="h-1.5" />;
        }
        return (
          <div key={lIdx} className="whitespace-pre-wrap break-words">
            {renderInlineFormatted(line, inlineCtx)}
          </div>
        );
      })}
    </div>
  );
}

const ThinkingBlock: React.FC<{
  summary: string;
  body: string;
  labels: Labels;
  inlineCtx: InlineContext;
  onSendCodeToPrism?: (lang: string, code: string, autoRun: boolean) => void;
  id: string;
}> = ({ summary, body, labels, inlineCtx, onSendCodeToPrism, id }) => (
  <details className="group/think my-2.5 rounded-xl border border-purple-200/80 dark:border-purple-400/20 bg-purple-50/55 dark:bg-purple-500/[0.07] overflow-hidden">
    <summary className="flex items-center gap-1.5 px-3.5 py-2 cursor-pointer select-none text-[11.5px] font-medium text-purple-700 dark:text-purple-300 list-none [&::-webkit-details-marker]:hidden">
      <ChevronRight size={12} className="transition-transform group-open/think:rotate-90" />
      <Brain size={12} />
      <span>{summary.replace(/^💭\s*/u, '') || labels.thinking}</span>
    </summary>
    <div className="px-3.5 pb-3 pt-1.5 text-slate-600 dark:text-slate-300 border-t border-purple-200/60 dark:border-purple-400/15">
      {renderMarkdown(body, labels, inlineCtx, onSendCodeToPrism, id)}
    </div>
  </details>
);

export const MessageBubble: React.FC<MessageBubbleProps> = React.memo(
  ({
    message,
    onOpenLocalFile,
    onSendCodeToPrism,
    onSelectConversation,
    onOpenExternalUrl,
    highlightQuery,
  }) => {
    const { t } = useI18n();
    const [copied, setCopied] = useState(false);
    const timer = useRef<ReturnType<typeof setTimeout>>();
    useEffect(() => () => clearTimeout(timer.current), []);

    const labels = useMemo<Labels>(
      () => ({
        copyCode: t.chat.copyCode,
        copiedCode: t.chat.copiedCode,
        thinking: t.chat.thinking,
        sendCodeToPrism: t.chat.sendCodeToPrism,
        sendCodeToPrismTooltip: t.chat.sendCodeToPrismTooltip,
        runCodeInPrism: t.chat.runCodeInPrism,
        runCodeInPrismTooltip: t.chat.runCodeInPrismTooltip,
      }),
      [t]
    );

    const inlineCtx = useMemo<InlineContext>(
      () => ({
        onOpenLocalFile,
        onSelectConversation,
        onOpenExternalUrl,
        openFileTooltipTemplate: t.chat.openFileInPrismTooltip,
        jumpConvTooltipTemplate: t.chat.jumpConversationTooltip,
        highlightQuery,
      }),
      [
        onOpenLocalFile,
        onSelectConversation,
        onOpenExternalUrl,
        t.chat.openFileInPrismTooltip,
        t.chat.jumpConversationTooltip,
        highlightQuery,
      ]
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
              inlineCtx={inlineCtx}
              onSendCodeToPrism={onSendCodeToPrism}
              id={`d-${idx}`}
            />
          ) : (
            <React.Fragment key={`m-${idx}`}>
              {renderMarkdown(
                segment.text,
                labels,
                inlineCtx,
                onSendCodeToPrism,
                `m-${idx}`
              )}
            </React.Fragment>
          )
        ),
      [message.content, labels, inlineCtx, onSendCodeToPrism]
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
        className={`p-4 md:px-5 md:py-4 rounded-2xl border ${meta.cardClass} group transition-colors duration-150`}
      >
        <div className={`flex items-center justify-between ${meta.headerClass}`}>
          <div className="flex items-center gap-2 min-w-0">
            <div
              className={`px-2.5 py-0.5 rounded-full border text-[11px] font-medium flex items-center gap-1.5 ${meta.badgeClass}`}
            >
              {meta.icon}
              <span>{meta.label}</span>
            </div>

            {message.model && (
              <span className="text-[10.5px] text-slate-500 dark:text-slate-400 bg-slate-100/90 dark:bg-slate-800/80 px-2 py-0.5 rounded-full border border-slate-200/80 dark:border-slate-700/75 font-mono truncate">
                {message.model}
              </span>
            )}

            {message.created_at && (
              <span className="text-[11px] text-slate-400 dark:text-slate-500 tracking-tight font-mono">
                {formatDateTime(message.created_at)}
              </span>
            )}
          </div>

          <button
            onClick={handleCopy}
            title={t.chat.copyMessageTooltip}
            className="opacity-0 group-hover:opacity-100 focus:opacity-100 flex items-center gap-1.5 text-[11px] text-slate-500 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white px-2.5 py-0.5 rounded-lg bg-white/80 dark:bg-slate-800/80 hover:bg-slate-100 dark:hover:bg-slate-700 border border-slate-200/85 dark:border-slate-700/80 shadow-xs transition-all"
          >
            {copied ? <Check size={11} className="text-emerald-600 dark:text-emerald-400" /> : <Copy size={11} />}
            <span>{copied ? t.chat.copiedCode : t.chat.copyMessage}</span>
          </button>
        </div>

        <div className="text-[13.5px] markdown-body select-text leading-[1.72]">{body}</div>
      </div>
    );
  }
);

MessageBubble.displayName = 'MessageBubble';
