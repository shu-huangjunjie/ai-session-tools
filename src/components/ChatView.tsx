import React, { useState, useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import {
  ArrowRightLeft,
  BookOpen,
  Braces,
  ChevronDown,
  Code2,
  Download,
  Edit2,
  FileText,
  Folder,
  FolderPlus,
  Layers,
  MessageSquare,
  RefreshCw,
  Search,
  Sparkles,
  Trash2,
  Bot,
  X,
} from 'lucide-react';

import { CanonicalMessage, Conversation, PrismOpenMode } from '../types';
import { extractProjectName, formatDateTime } from '../utils/formatters';
import { Button } from './common/Button';
import { DropdownMenu } from './common/DropdownMenu';
import { MessageBubble } from './MessageBubble';
import { useI18n } from '../i18n';

interface ChatViewProps {
  conversation: Conversation | null;
  messages: CanonicalMessage[];
  isLoading: boolean;
  onRename: (id: string, currentTitle: string) => void;
  onDelete: (id: string, title: string) => void;
  onExportMarkdown: (id: string, title: string, projectPath?: string | null) => void;
  onExportJson: (id: string, title: string, projectPath?: string | null) => void;
  onOpenFolder: (path: string) => void;
  onOpenVSCode: (path: string) => void;
  onOpenPrismProject?: (path: string, mode?: PrismOpenMode) => void;
  onOpenFileInPrism?: (filePath: string, line?: number, projectPath?: string | null) => void;
  onSendCodeToPrism?: (
    lang: string,
    code: string,
    title: string,
    projectPath?: string | null,
    autoRun?: boolean
  ) => void;
  onReadConversationInPrism?: (conversationId: string) => void;
  onHandoff?: (conversation: Conversation) => void;
  onSelectConversation?: (conversationId: string) => void;
  onOpenExternalUrl?: (url: string) => void;
}

const BATCH_SIZE = 60;
type RoleFilter = 'all' | 'user' | 'assistant';

export const ChatView: React.FC<ChatViewProps> = ({
  conversation,
  messages,
  isLoading,
  onRename,
  onDelete,
  onExportMarkdown,
  onExportJson,
  onOpenFolder,
  onOpenVSCode,
  onOpenPrismProject,
  onOpenFileInPrism,
  onSendCodeToPrism,
  onReadConversationInPrism,
  onHandoff,
  onSelectConversation,
  onOpenExternalUrl,
}) => {
  const { t, interpolate } = useI18n();
  // Paging for long conversations: render the most recent BATCH_SIZE messages initially
  const [renderedCount, setRenderedCount] = useState<number>(BATCH_SIZE);
  const [roleFilter, setRoleFilter] = useState<RoleFilter>('all');
  const [inChatQuery, setInChatQuery] = useState<string>('');
  const scrollRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  // Scroll height before "load earlier", so the viewport stays on the same message.
  const anchorHeight = useRef<number | null>(null);

  useEffect(() => {
    // Reset paging & in-chat filters whenever conversation switches
    setRenderedCount(BATCH_SIZE);
    setRoleFilter('all');
    setInChatQuery('');
  }, [conversation?.id]);

  // Ctrl+F inside ChatView focuses the in-conversation message filter
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f' && conversation) {
        e.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [conversation]);

  // Open each conversation at its latest message, like a chat client.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && !isLoading) el.scrollTop = el.scrollHeight;
  }, [conversation?.id, isLoading, messages]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && anchorHeight.current !== null) {
      el.scrollTop += el.scrollHeight - anchorHeight.current;
      anchorHeight.current = null;
    }
  }, [renderedCount]);

  const isFiltering = roleFilter !== 'all' || inChatQuery.trim() !== '';

  const filteredMessages = useMemo(() => {
    const q = inChatQuery.trim().toLowerCase();
    if (!isFiltering) return messages;
    return messages.filter((m) => {
      if (roleFilter === 'user' && m.role !== 'user') return false;
      if (roleFilter === 'assistant' && m.role !== 'assistant') return false;
      if (q && !m.content.toLowerCase().includes(q)) return false;
      return true;
    });
  }, [messages, roleFilter, inChatQuery, isFiltering]);

  const visibleMessages = useMemo(() => {
    if (isFiltering) {
      return filteredMessages;
    }
    if (messages.length <= renderedCount) {
      return messages;
    }
    const startIndex = messages.length - renderedCount;
    return messages.slice(startIndex);
  }, [messages, filteredMessages, renderedCount, isFiltering]);

  const remainingEarlierCount = isFiltering
    ? 0
    : Math.max(0, messages.length - renderedCount);

  const handleLoadEarlier = () => {
    anchorHeight.current = scrollRef.current?.scrollHeight ?? null;
    setRenderedCount((prev) => prev + BATCH_SIZE);
  };

  if (!conversation) {
    return (
      <main className="flex-1 h-full flex flex-col items-center justify-center bg-transparent text-slate-500 p-8 select-none transition-colors">
        <div className="relative mb-5">
          <div className="absolute -inset-3 rounded-3xl bg-gradient-to-tr from-blue-500/15 via-indigo-500/10 to-sky-400/15 blur-xl" aria-hidden="true" />
          <div className="relative w-16 h-16 rounded-2xl bg-gradient-to-br from-white via-slate-50 to-blue-50/75 dark:from-[#1b2740] dark:via-[#151f33] dark:to-[#11192a] border border-slate-200/90 dark:border-slate-700/80 flex items-center justify-center text-blue-600 dark:text-sky-400 shadow-[0_10px_28px_rgba(37,99,235,0.12)] dark:shadow-[0_12px_32px_rgba(0,0,0,0.45)]">
            <MessageSquare size={26} strokeWidth={1.85} />
          </div>
        </div>
        <h3 className="text-[15px] font-semibold tracking-tight text-slate-900 dark:text-slate-100 mb-1.5">
          {t.chat.emptyTitle}
        </h3>
        <p className="text-xs text-slate-500 dark:text-slate-400 max-w-sm text-center leading-relaxed">
          {t.chat.emptySubtitle}
        </p>
      </main>
    );
  }

  const projectName = extractProjectName(conversation.project_path);

  return (
    <main className="flex-1 h-full flex flex-col bg-transparent overflow-hidden transition-colors">
      {/* Top Conversation Header */}
      <div className="px-6 py-3 bg-white/80 dark:bg-[#121a2a]/82 backdrop-blur-xl border-b border-slate-200/75 dark:border-slate-800/80 flex flex-col gap-2.5 flex-shrink-0 z-10 transition-colors">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <h2
                className="text-[15.5px] font-semibold tracking-tight text-slate-900 dark:text-white truncate leading-snug"
                title={conversation.title}
              >
                {conversation.title}
              </h2>
              <button
                onClick={() => onRename(conversation.id, conversation.title)}
                className="text-slate-400 hover:text-slate-800 dark:hover:text-slate-200 transition-colors p-1 rounded-lg hover:bg-slate-100 dark:hover:bg-slate-800/80 flex-shrink-0"
                title={t.chat.renameTooltip}
              >
                <Edit2 size={12.5} />
              </button>
            </div>

            <div className="flex items-center gap-2 text-[11px] text-slate-500 dark:text-slate-400 mt-1 flex-wrap">
              {conversation.source_kind === 'antigravity' ? (
                <div className="flex items-center gap-1 bg-purple-50 dark:bg-purple-500/15 text-purple-600 dark:text-purple-300 border border-purple-200/80 dark:border-purple-400/25 px-2 py-0.5 rounded-full font-medium">
                  <Sparkles size={10} />
                  <span>Antigravity</span>
                </div>
              ) : conversation.source_kind === 'claude' ? (
                <div className="flex items-center gap-1 bg-amber-50 dark:bg-amber-500/15 text-amber-700 dark:text-amber-300 border border-amber-200/80 dark:border-amber-400/25 px-2 py-0.5 rounded-full font-medium">
                  <Bot size={10} />
                  <span>Claude</span>
                </div>
              ) : conversation.source_kind === 'codex_main' ? (
                <div className="flex items-center gap-1 bg-blue-50 dark:bg-blue-500/15 text-blue-600 dark:text-sky-300 border border-blue-200/80 dark:border-blue-400/25 px-2 py-0.5 rounded-full font-medium">
                  <span>Codex</span>
                </div>
              ) : null}

              {conversation.project_path && (
                <button
                  type="button"
                  onClick={() =>
                    onOpenPrismProject
                      ? onOpenPrismProject(conversation.project_path!, 'new-workspace')
                      : onOpenFolder(conversation.project_path!)
                  }
                  title={`${conversation.project_path} — ${t.chat.openPrismTooltip}`}
                  className="flex items-center gap-1.5 bg-slate-100/85 dark:bg-[#1c263b] hover:bg-blue-50 dark:hover:bg-blue-500/15 px-2.5 py-0.5 rounded-full border border-slate-200/85 dark:border-slate-700/75 hover:border-blue-200 dark:hover:border-blue-400/30 text-slate-700 dark:text-slate-200 hover:text-blue-600 dark:hover:text-sky-300 transition-colors"
                >
                  <Folder size={11} className="text-amber-500 dark:text-amber-400 flex-shrink-0" />
                  <span className="truncate max-w-[280px]">{projectName}</span>
                </button>
              )}

              <span>
                {interpolate(t.chat.updatedAt, { time: formatDateTime(conversation.updated_at) })}
              </span>
              <span className="text-slate-300 dark:text-slate-600">·</span>
              <span className="font-mono text-slate-500 dark:text-slate-400">
                {interpolate(t.chat.messageCount, { count: messages.length })}
              </span>
            </div>
          </div>

          {/* Action Toolbar */}
          <div className="flex items-center gap-2 flex-shrink-0">
            {conversation.project_path ? (
              <>
                {onOpenPrismProject && (
                  <div className="inline-flex items-stretch rounded-lg shadow-[0_1px_2px_rgba(29,78,216,0.2)]">
                    <Button
                      variant="primary"
                      icon={<Sparkles size={13} />}
                      onClick={() =>
                        onOpenPrismProject(conversation.project_path!, 'new-workspace')
                      }
                      title={t.chat.openPrismTooltip}
                      className="rounded-r-none border-r-0 shadow-none"
                    >
                      {t.chat.openPrism}
                    </Button>
                    <DropdownMenu
                      variant="primary"
                      title={t.chat.openPrismTooltip}
                      icon={<ChevronDown size={12} />}
                      triggerClassName="rounded-l-none border-l border-white/25 px-2 shadow-none"
                      items={[
                        {
                          key: 'prism-new-ws',
                          label: t.chat.prismNewWorkspace,
                          icon: <Layers size={13} />,
                          onSelect: () =>
                            onOpenPrismProject(conversation.project_path!, 'new-workspace'),
                        },
                        {
                          key: 'prism-parallel',
                          label: t.chat.prismParallelRoot,
                          icon: <FolderPlus size={13} />,
                          onSelect: () =>
                            onOpenPrismProject(conversation.project_path!, 'parallel-root'),
                        },
                        ...(onReadConversationInPrism
                          ? [
                              {
                                key: 'prism-read',
                                label: t.chat.prismReadSession,
                                icon: <BookOpen size={13} />,
                                onSelect: () => onReadConversationInPrism(conversation.id),
                              },
                            ]
                          : []),
                      ]}
                    />
                  </div>
                )}
                <Button
                  icon={<Folder size={13} />}
                  onClick={() => onOpenFolder(conversation.project_path!)}
                  title={t.chat.openFolderTooltip}
                >
                  {t.chat.openFolder}
                </Button>
                <Button
                  icon={<Code2 size={13} />}
                  onClick={() => onOpenVSCode(conversation.project_path!)}
                  title={t.chat.openVSCodeTooltip}
                >
                  {t.chat.openVSCode}
                </Button>
              </>
            ) : (
              onReadConversationInPrism && (
                <Button
                  variant="primary"
                  icon={<Sparkles size={13} />}
                  onClick={() => onReadConversationInPrism(conversation.id)}
                  title={t.chat.prismReadSession}
                >
                  {t.chat.openPrism}
                </Button>
              )
            )}

            {onHandoff && (
              <Button
                variant="accent"
                icon={<ArrowRightLeft size={13} />}
                onClick={() => onHandoff(conversation)}
                title={t.chat.handoffTooltip}
              >
                {t.chat.handoff}
              </Button>
            )}

            <DropdownMenu
              label={t.chat.export}
              title={t.chat.exportTooltip}
              icon={<Download size={13} />}
              trailing={<ChevronDown size={12} className="-mr-0.5 text-slate-400" />}
              items={[
                ...(onReadConversationInPrism
                  ? [
                      {
                        key: 'prism-note',
                        label: t.chat.prismReadSession,
                        icon: <Sparkles size={13} />,
                        onSelect: () => onReadConversationInPrism(conversation.id),
                      },
                    ]
                  : []),
                {
                  key: 'markdown',
                  label: t.chat.saveMarkdown,
                  icon: <FileText size={13} />,
                  onSelect: () =>
                    onExportMarkdown(conversation.id, conversation.title, conversation.project_path),
                },
                {
                  key: 'json',
                  label: t.chat.exportJson,
                  icon: <Braces size={13} />,
                  onSelect: () =>
                    onExportJson(conversation.id, conversation.title, conversation.project_path),
                },
              ]}
            />

            <Button
              variant="danger"
              icon={<Trash2 size={13} />}
              onClick={() => onDelete(conversation.id, conversation.title)}
              title={t.chat.deleteTooltip}
              aria-label={t.chat.deleteTooltip}
              className="px-2"
            />
          </div>
        </div>

        {/* Compact In-Conversation Filter & Search Bar */}
        {messages.length > 0 && (
          <div className="flex items-center justify-between gap-3 pt-0.5">
            <div className="inline-flex items-center gap-0.5 p-[2.5px] rounded-xl bg-slate-100/95 dark:bg-[#0f1522] border border-slate-200/75 dark:border-slate-800/85 text-[11px]">
              {(
                [
                  { key: 'all', label: t.chat.filterAllRoles },
                  { key: 'user', label: t.chat.filterUserOnly },
                  { key: 'assistant', label: t.chat.filterAssistantOnly },
                ] as const
              ).map((opt) => (
                <button
                  key={opt.key}
                  type="button"
                  onClick={() => setRoleFilter(opt.key)}
                  className={`px-2.5 py-0.5 rounded-[8px] font-medium transition-all ${
                    roleFilter === opt.key
                      ? 'bg-white dark:bg-[#202b40] text-slate-900 dark:text-white shadow-[0_1px_2px_rgba(15,23,42,0.06)] ring-1 ring-slate-200/70 dark:ring-slate-700/70'
                      : 'text-slate-500 dark:text-slate-400 hover:text-slate-800 dark:hover:text-slate-200'
                  }`}
                >
                  {opt.label}
                </button>
              ))}
            </div>

            <div className="flex items-center gap-2 min-w-0">
              {isFiltering && (
                <span className="text-[11px] font-mono text-blue-600 dark:text-sky-400 flex-shrink-0">
                  {interpolate(t.chat.inChatMatchCount, { count: filteredMessages.length })}
                </span>
              )}
              <div className="relative w-56">
                <Search
                  size={12}
                  className="absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-400 pointer-events-none"
                />
                <input
                  ref={searchInputRef}
                  type="search"
                  value={inChatQuery}
                  onChange={(e) => setInChatQuery(e.target.value)}
                  placeholder={t.chat.inChatSearchPlaceholder}
                  className="w-full bg-slate-50 dark:bg-[#101725] text-slate-800 dark:text-slate-100 text-[11px] pl-7 pr-12 py-1 rounded-lg border border-slate-200/85 dark:border-slate-700/75 focus:border-blue-500 focus:bg-white dark:focus:bg-[#162033] focus:ring-2 focus:ring-blue-500/15 focus:outline-none placeholder-slate-400 dark:placeholder-slate-500 transition-all"
                />
                {inChatQuery ? (
                  <button
                    type="button"
                    onClick={() => setInChatQuery('')}
                    title={t.chat.clearFilter}
                    className="absolute right-1.5 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-800 dark:hover:text-white p-0.5"
                  >
                    <X size={11} />
                  </button>
                ) : (
                  <kbd className="absolute right-2 top-1/2 -translate-y-1/2 text-[9.5px] font-mono text-slate-400 dark:text-slate-500 px-1 rounded border border-slate-200/80 dark:border-slate-700/80 bg-white/80 dark:bg-slate-800/70 pointer-events-none">
                    Ctrl F
                  </kbd>
                )}
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Message List Scroll Area */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-5 md:px-8 py-6">
        <div className="max-w-[960px] mx-auto w-full space-y-4">
          {isLoading ? (
            <div className="flex items-center justify-center py-24 text-slate-500 gap-2.5 text-xs">
              <RefreshCw size={15} className="animate-spin text-blue-600 dark:text-sky-400" />
              {t.chat.loadingMessages}
            </div>
          ) : (
            <>
              {/* Load Older Messages Capsule Button */}
              {remainingEarlierCount > 0 && (
                <div className="flex justify-center my-3">
                  <button
                    onClick={handleLoadEarlier}
                    className="px-5 py-2 rounded-full bg-white dark:bg-[#192338] hover:bg-slate-50 dark:hover:bg-[#22304a] text-slate-700 dark:text-slate-200 border border-slate-200/90 dark:border-slate-700/80 text-xs font-medium flex items-center gap-2 transition-all shadow-sm active:scale-[0.98]"
                  >
                    <span>
                      {interpolate(t.chat.loadEarlier, { count: remainingEarlierCount })}
                    </span>
                  </button>
                </div>
              )}

              {visibleMessages.length === 0 ? (
                <div className="text-center py-20 text-slate-400 dark:text-slate-500 text-xs">
                  {isFiltering ? t.common.noResults : t.chat.noMessages}
                </div>
              ) : (
                visibleMessages.map((msg) => (
                  <MessageBubble
                    key={msg.id}
                    message={msg}
                    highlightQuery={inChatQuery}
                    onOpenLocalFile={
                      onOpenFileInPrism
                        ? (filePath, line) =>
                            onOpenFileInPrism(filePath, line, conversation.project_path)
                        : undefined
                    }
                    onSendCodeToPrism={
                      onSendCodeToPrism
                        ? (lang, code, autoRun) =>
                            onSendCodeToPrism(
                              lang,
                              code,
                              conversation.title,
                              conversation.project_path,
                              autoRun
                            )
                        : undefined
                    }
                    onSelectConversation={onSelectConversation}
                    onOpenExternalUrl={onOpenExternalUrl}
                  />
                ))
              )}
            </>
          )}
        </div>
      </div>
    </main>
  );
};
