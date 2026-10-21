import React, { useState, useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import {
  Code2,
  Download,
  Edit2,
  FileText,
  Folder,
  MessageSquare,
  RefreshCw,
  Sparkles,
  Trash2,
  Bot,
} from 'lucide-react';

import { CanonicalMessage, Conversation } from '../types';
import { extractProjectName, formatDateTime } from '../utils/formatters';
import { Button } from './common/Button';
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
}

const BATCH_SIZE = 60;

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
}) => {
  const { t, interpolate } = useI18n();
  // Paging for long conversations: render the most recent BATCH_SIZE messages initially
  const [renderedCount, setRenderedCount] = useState<number>(BATCH_SIZE);
  const scrollRef = useRef<HTMLDivElement>(null);
  // Scroll height before "load earlier", so the viewport stays on the same message.
  const anchorHeight = useRef<number | null>(null);

  useEffect(() => {
    // Reset paging whenever conversation switches
    setRenderedCount(BATCH_SIZE);
  }, [conversation?.id]);

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

  const visibleMessages = useMemo(() => {
    if (messages.length <= renderedCount) {
      return messages;
    }
    const startIndex = messages.length - renderedCount;
    return messages.slice(startIndex);
  }, [messages, renderedCount]);

  const remainingEarlierCount = Math.max(0, messages.length - renderedCount);

  const handleLoadEarlier = () => {
    anchorHeight.current = scrollRef.current?.scrollHeight ?? null;
    setRenderedCount((prev) => prev + BATCH_SIZE);
  };

  if (!conversation) {
    return (
      <main className="flex-1 h-full flex flex-col items-center justify-center bg-[#f5f5f7] dark:bg-black text-[#86868b] p-8 select-none transition-colors">
        <div className="w-16 h-16 rounded-3xl bg-black/[0.04] dark:bg-gradient-to-b dark:from-white/[0.08] dark:to-white/[0.02] border border-black/[0.06] dark:border-white/[0.08] flex items-center justify-center text-[#86868b] mb-4 shadow-xl dark:shadow-2xl">
          <MessageSquare size={26} className="text-[#86868b] dark:text-[#a1a1a6]" />
        </div>
        <h3 className="text-sm font-semibold tracking-tight text-[#1d1d1f] dark:text-[#f5f5f7] mb-1">
          {t.chat.emptyTitle}
        </h3>
        <p className="text-xs text-[#86868b] max-w-sm text-center leading-relaxed">
          {t.chat.emptySubtitle}
        </p>
      </main>
    );
  }

  const projectName = extractProjectName(conversation.project_path);

  return (
    <main className="flex-1 h-full flex flex-col bg-[#f5f5f7] dark:bg-[#050507] overflow-hidden transition-colors">
      {/* Apple Style Top Conversation Header */}
      <div className="px-6 py-3.5 backdrop-blur-xl bg-white/75 dark:bg-black/60 border-b border-black/[0.06] dark:border-white/[0.08] flex flex-col gap-2 flex-shrink-0 z-10 transition-colors">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <h2
                className="text-base font-semibold tracking-tight bg-gradient-to-r from-[#1d1d1f] via-[#2d2d30] to-[#515154] dark:from-white dark:via-[#f5f5f7] dark:to-[#a1a1a6] bg-clip-text text-transparent truncate leading-snug"
                title={conversation.title}
              >
                {conversation.title}
              </h2>
              <button
                onClick={() => onRename(conversation.id, conversation.title)}
                className="text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-[#f5f5f7] transition-colors p-1 rounded-full hover:bg-black/[0.05] dark:hover:bg-white/[0.06]"
                title={t.chat.renameTooltip}
              >
                <Edit2 size={12} />
              </button>
            </div>

            <div className="flex items-center gap-2.5 text-[11px] text-[#86868b] mt-1 flex-wrap">
              {conversation.source_kind === 'antigravity' ? (
                <div className="flex items-center gap-1 bg-purple-500/10 text-purple-600 dark:text-purple-400 border border-purple-500/20 px-2 py-0.5 rounded-full font-medium">
                  <Sparkles size={10} />
                  <span>Antigravity</span>
                </div>
              ) : conversation.source_kind === 'claude' ? (
                <div className="flex items-center gap-1 bg-amber-500/10 text-amber-700 dark:text-amber-300 border border-amber-500/20 px-2 py-0.5 rounded-full font-medium">
                  <Bot size={10} />
                  <span>Claude</span>
                </div>
              ) : conversation.source_kind === 'codex_main' ? (
                <div className="flex items-center gap-1 bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20 px-2 py-0.5 rounded-full font-medium">
                  <span>Codex</span>
                </div>
              ) : null}

              {conversation.project_path && (
                <div className="flex items-center gap-1.5 bg-black/[0.04] dark:bg-white/[0.05] px-2.5 py-0.5 rounded-full border border-black/[0.06] dark:border-white/[0.08] text-[#1d1d1f] dark:text-[#d2d2d7]">
                  <Folder size={11} className="text-[#86868b]" />
                  <span className="truncate max-w-[280px]" title={conversation.project_path}>
                    {projectName}
                  </span>
                </div>
              )}

              <span>{interpolate(t.chat.updatedAt, { time: formatDateTime(conversation.updated_at) })}</span>
              <span>·</span>
              <span className="font-mono">{interpolate(t.chat.messageCount, { count: messages.length })}</span>
            </div>

          </div>

          {/* Apple Pill Action Toolbar */}
          <div className="flex items-center gap-1.5 flex-shrink-0">
            {conversation.project_path && (
              <>
                <Button
                  variant="secondary"
                  size="sm"
                  icon={<Folder size={12} className="text-[#86868b]" />}
                  onClick={() => onOpenFolder(conversation.project_path!)}
                  title={t.chat.openFolderTooltip}
                >
                  {t.chat.openFolder}
                </Button>
                <Button
                  variant="secondary"
                  size="sm"
                  icon={<Code2 size={12} className="text-[#0071e3] dark:text-[#2997ff]" />}
                  onClick={() => onOpenVSCode(conversation.project_path!)}
                  title={t.chat.openVSCodeTooltip}
                >
                  {t.chat.openVSCode}
                </Button>
              </>
            )}

            <Button
              variant="secondary"
              size="sm"
              icon={<FileText size={12} className="text-[#86868b]" />}
              onClick={() => onExportMarkdown(conversation.id, conversation.title, conversation.project_path)}
              title={t.chat.saveMarkdownTooltip}
            >
              {t.chat.saveMarkdown}
            </Button>

            <Button
              variant="ghost"
              size="sm"
              icon={<Download size={12} className="text-[#86868b]" />}
              onClick={() => onExportJson(conversation.id, conversation.title, conversation.project_path)}
              title={t.chat.exportJsonTooltip}
            >
              {t.chat.exportJson}
            </Button>

            <Button
              variant="danger"
              size="sm"
              icon={<Trash2 size={12} />}
              onClick={() => onDelete(conversation.id, conversation.title)}
              title={t.chat.deleteTooltip}
            >
              {t.chat.delete}
            </Button>
          </div>
        </div>
      </div>

      {/* Message List Scroll Area */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 md:px-8 py-6 space-y-4">
        {isLoading ? (
          <div className="flex items-center justify-center py-24 text-[#86868b] gap-2.5 text-xs">
            <RefreshCw size={15} className="animate-spin text-[#0071e3]" />
            {t.chat.loadingMessages}
          </div>
        ) : (
          <>
            {/* Load Older Messages Capsule Button */}
            {remainingEarlierCount > 0 && (
              <div className="flex justify-center my-3">
                <button
                  onClick={handleLoadEarlier}
                  className="px-5 py-2 rounded-full bg-white dark:bg-white/[0.05] hover:bg-black/[0.03] dark:hover:bg-white/[0.1] text-[#1d1d1f] dark:text-[#ededf0] hover:text-black dark:hover:text-white border border-black/[0.08] dark:border-white/[0.08] text-xs font-medium flex items-center gap-2 transition-all shadow-sm active:scale-[0.98]"
                >
                  <span>{interpolate(t.chat.loadEarlier, { count: remainingEarlierCount })}</span>
                </button>
              </div>
            )}

            {visibleMessages.length === 0 ? (
              <div className="text-center py-20 text-[#86868b] text-xs">
                {t.chat.noMessages}
              </div>
            ) : (
              visibleMessages.map((msg) => <MessageBubble key={msg.id} message={msg} />)
            )}
          </>
        )}
      </div>
    </main>
  );
};
