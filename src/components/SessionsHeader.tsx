import React from 'react';
import {
  Database,
  DatabaseZap,
  Ellipsis,
  ExternalLink,
  FolderSync,
  Loader2,
  Send,
  UploadCloud,
} from 'lucide-react';
import { Button } from './common/Button';
import { DropdownMenu } from './common/DropdownMenu';
import { PageHeader } from './common/PageHeader';
import { Segmented } from './common/Segmented';
import { useI18n } from '../i18n';

export type SourceCategory = 'codex' | 'antigravity' | 'claude';

const SOURCE_NAME: Record<SourceCategory, string> = { codex: 'Codex', antigravity: 'Antigravity', claude: 'Claude' };
export const SOURCE_COLOR: Record<SourceCategory, string> = { codex: '#0071e3', antigravity: '#af52de', claude: '#d97706' };

interface SessionsHeaderProps {
  activeSource: SourceCategory;
  onSourceChange: (source: SourceCategory) => void;
  codexCount: number;
  /** Conversations Codex lists; absent when Codex keeps no project state. */
  codexVisibleCount?: number;
  antigravityCount: number;
  claudeCount: number;
  selectionCount: number;
  onScan: () => void;
  isScanning: boolean;
  onPublish: () => void;
  onOpenClient: () => void;
  isOpeningClient: boolean;
  onClearDatabase: () => void;
  isClearingDatabase: boolean;
  onOpenImport: () => void;
  onBackup: () => void;
  showArchived: boolean;
  onShowArchivedChange: (show: boolean) => void;
}

/** Top bar of the sessions page: source tabs, then scan, publish and database actions. */
export const SessionsHeader: React.FC<SessionsHeaderProps> = ({
  activeSource,
  onSourceChange,
  codexCount,
  codexVisibleCount,
  antigravityCount,
  claudeCount,
  selectionCount,
  onScan,
  isScanning,
  onPublish,
  onOpenClient,
  isOpeningClient,
  onClearDatabase,
  isClearingDatabase,
  onOpenImport,
  onBackup,
  showArchived,
  onShowArchivedChange,
}) => {
  const { t, interpolate } = useI18n();
  const name = SOURCE_NAME[activeSource];

  const scanTooltip = {
    codex: t.sidebar.scanCodexTooltip,
    antigravity: t.sidebar.scanAntigravityTooltip,
    claude: t.sidebar.scanClaudeTooltip,
  }[activeSource];
  const publish = {
    codex: { label: t.sidebar.publishToCodex, tooltip: t.sidebar.publishToCodexTooltip, icon: <Send size={13} /> },
    antigravity: { label: t.sidebar.updateToAntigravity, tooltip: t.sidebar.updateToAntigravityTooltip, icon: <UploadCloud size={13} /> },
    claude: { label: t.sidebar.updateToClaudeClient, tooltip: t.sidebar.updateToClaudeClientTooltip, icon: <UploadCloud size={13} /> },
  }[activeSource];

  return (
    <PageHeader
      title={t.sessions.title}
      tabs={
        <Segmented
          value={activeSource}
          onChange={onSourceChange}
          items={[
            {
              key: 'codex',
              label: 'Codex',
              dot: SOURCE_COLOR.codex,
              count: codexVisibleCount === undefined ? codexCount : `${codexVisibleCount}/${codexCount}`,
              title:
                codexVisibleCount === undefined
                  ? 'Codex'
                  : interpolate(t.header.codexCountTooltip, { visible: codexVisibleCount, total: codexCount }),
            },
            { key: 'antigravity', label: 'Antigravity', dot: SOURCE_COLOR.antigravity, count: antigravityCount, title: 'Antigravity' },
            { key: 'claude', label: 'Claude', dot: SOURCE_COLOR.claude, count: claudeCount, title: 'Claude' },
          ]}
        />
      }
    >
      <Button
        icon={isScanning ? <Loader2 size={13} className="animate-spin" /> : <FolderSync size={13} />}
        onClick={onScan}
        disabled={isScanning}
        title={scanTooltip}
      >
        {isScanning ? t.sidebar.scanning : t.sessions.scan}
      </Button>
      <Button variant="primary" icon={publish.icon} onClick={onPublish} title={publish.tooltip}>
        {publish.label}
        {selectionCount > 0 && ` (${selectionCount})`}
      </Button>
      <Button
        icon={isOpeningClient ? <Loader2 size={13} className="animate-spin" /> : <ExternalLink size={13} />}
        onClick={onOpenClient}
        disabled={isOpeningClient}
        title={interpolate(t.header.openClientTooltip, { name })}
      >
        {interpolate(t.header.openClient, { name })}
      </Button>
      <div className="h-5 w-px bg-black/[0.08] dark:bg-white/[0.1] mx-1" />
      <Button
        variant="danger"
        icon={<DatabaseZap size={13} />}
        onClick={onClearDatabase}
        disabled={isClearingDatabase}
        title={t.header.clearDatabaseTooltip}
      >
        {t.header.clearDatabase}
      </Button>
      <DropdownMenu
        variant="ghost"
        title={t.sessions.more}
        icon={<Ellipsis size={16} />}
        items={[
          { key: 'import', label: t.sessions.importConversations, icon: <UploadCloud size={13} />, onSelect: onOpenImport },
          { key: 'backup', label: t.sessions.backupDatabase, icon: <Database size={13} />, onSelect: onBackup },
          { key: 'archived', label: t.sidebar.showArchived, checked: showArchived, onSelect: () => onShowArchivedChange(!showArchived) },
        ]}
      />
    </PageHeader>
  );
};
