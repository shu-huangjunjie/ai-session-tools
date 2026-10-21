import React, { useEffect, useState } from 'react';
import { AlertCircle, CheckCircle2, FileUp, FolderOpen, RefreshCw, UploadCloud } from 'lucide-react';
import { errorMessage } from '../api/client';
import { ImportPreview } from '../types';
import { Button } from './common/Button';
import { Modal } from './common/Modal';
import { useI18n } from '../i18n';

interface ImportModalProps {
  isOpen: boolean;
  onClose: () => void;
  onPickFile: () => Promise<string | null>;
  onPreview: (filePath: string) => Promise<ImportPreview>;
  onExecuteImport: (filePath: string) => Promise<void>;
}

export const ImportModal: React.FC<ImportModalProps> = ({
  isOpen,
  onClose,
  onPickFile,
  onPreview,
  onExecuteImport,
}) => {
  const { t, interpolate } = useI18n();
  const [filePath, setFilePath] = useState('');
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [isLoadingPreview, setIsLoadingPreview] = useState(false);
  const [isImporting, setIsImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (isOpen) {
      setFilePath('');
      setPreview(null);
      setError(null);
    }
  }, [isOpen]);

  const runPreview = async (path: string) => {
    if (!path.trim()) return;
    setIsLoadingPreview(true);
    setError(null);
    try {
      setPreview(await onPreview(path.trim()));
    } catch (e) {
      setError(errorMessage(e));
      setPreview(null);
    } finally {
      setIsLoadingPreview(false);
    }
  };

  const handleBrowse = async () => {
    try {
      const picked = await onPickFile();
      if (picked) {
        setFilePath(picked);
        await runPreview(picked);
      }
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const handleImport = async () => {
    if (!filePath.trim() || !preview) return;
    setIsImporting(true);
    setError(null);
    try {
      await onExecuteImport(filePath.trim());
      onClose();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setIsImporting(false);
    }
  };

  if (!isOpen) return null;

  return (
    <Modal
      isOpen={isOpen}
      onClose={isImporting ? () => undefined : onClose}
      title={t.modals.importTitle}
      subtitle={t.modals.importSubtitle}
      widthClass="max-w-xl"
    >
      <div className="flex flex-col gap-4 text-xs">
        <div>
          <label htmlFor="import-path" className="block text-xs font-semibold text-[#1d1d1f] dark:text-[#f5f5f7] mb-2">
            {t.modals.importPathLabel}
          </label>
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void runPreview(filePath);
            }}
          >
            <input
              id="import-path"
              type="text"
              value={filePath}
              onChange={(e) => {
                setFilePath(e.target.value);
                // A preview only describes the file it was made for.
                setPreview(null);
              }}
              placeholder={t.modals.importPathPlaceholder}
              className="flex-1 min-w-0 bg-black/[0.04] dark:bg-white/[0.05] text-[#1d1d1f] dark:text-[#ededf0] text-xs px-4 py-2 rounded-full border border-black/[0.08] dark:border-white/[0.08] focus:border-[#0071e3] focus:outline-none placeholder-[#86868b] transition-all"
            />
            <Button
              type="button"
              variant="secondary"
              size="sm"
              icon={<FolderOpen size={13} />}
              onClick={handleBrowse}
              disabled={isLoadingPreview || isImporting}
            >
              {t.common.browse}
            </Button>
            <Button
              type="submit"
              variant="secondary"
              size="sm"
              icon={isLoadingPreview ? <RefreshCw size={13} className="animate-spin" /> : <FileUp size={13} />}
              disabled={isLoadingPreview || isImporting || !filePath.trim()}
            >
              {t.modals.importPreview}
            </Button>
          </form>
        </div>

        {error && (
          <div className="p-3.5 bg-[#ff3b30]/10 dark:bg-[#ff453a]/10 border border-[#ff3b30]/25 dark:border-[#ff453a]/25 rounded-2xl text-[#d70015] dark:text-[#ff453a] flex items-start gap-2.5 leading-relaxed break-all">
            <AlertCircle size={15} className="flex-shrink-0 mt-0.5" />
            <span>{error}</span>
          </div>
        )}

        {preview && (
          <div className="p-4 bg-black/[0.02] dark:bg-white/[0.03] border border-black/[0.08] dark:border-white/[0.08] rounded-2xl flex flex-col gap-3">
            <div className="flex items-center justify-between pb-2.5 border-b border-black/[0.06] dark:border-white/[0.06]">
              <span className="font-semibold text-[#1d1d1f] dark:text-white flex items-center gap-2">
                <CheckCircle2 size={14} className="text-emerald-500 dark:text-emerald-400" />
                {t.modals.importValidated}
              </span>
              <span className="text-[10px] bg-black/[0.04] dark:bg-white/[0.06] text-[#86868b] px-2.5 py-0.5 rounded-full border border-black/[0.06] dark:border-white/[0.08] font-medium">
                {preview.source_type}
              </span>
            </div>

            <div className="grid grid-cols-2 gap-2 text-[11px]">
              <div>
                <span className="text-[#86868b]">{t.modals.importConversations}：</span>
                <strong className="text-[#1d1d1f] dark:text-white ml-1 font-semibold">
                  {preview.conversation_count}
                </strong>
              </div>
              <div>
                <span className="text-[#86868b]">{t.modals.importMessages}：</span>
                <strong className="text-[#1d1d1f] dark:text-white ml-1 font-semibold">{preview.message_count}</strong>
              </div>
            </div>

            {preview.sample_titles.length > 0 && (
              <div>
                <span className="text-[#86868b] text-[11px] block mb-1.5">{t.modals.importSamples}：</span>
                <div className="bg-black/[0.03] dark:bg-black/30 p-2.5 rounded-xl border border-black/[0.06] dark:border-white/[0.06] space-y-1">
                  {preview.sample_titles.map((title, idx) => (
                    <div key={idx} className="text-[#333336] dark:text-[#d2d2d7] truncate">
                      • {title}
                    </div>
                  ))}
                </div>
              </div>
            )}

            {preview.warning_count > 0 && (
              <details className="text-[11px] text-amber-700 dark:text-amber-300">
                <summary className="cursor-pointer select-none">
                  {interpolate(t.modals.importWarnings, { count: preview.warning_count })}
                </summary>
                <ul className="mt-1.5 space-y-0.5 max-h-28 overflow-y-auto pl-4 list-disc">
                  {preview.warnings.slice(0, 50).map((w, idx) => (
                    <li key={idx}>{w}</li>
                  ))}
                </ul>
              </details>
            )}
          </div>
        )}

        <div className="flex items-center justify-end gap-2 pt-3 border-t border-black/[0.08] dark:border-white/[0.08]">
          <Button variant="ghost" size="sm" onClick={onClose} disabled={isImporting}>
            {t.common.cancel}
          </Button>
          <Button
            variant="primary"
            size="sm"
            icon={isImporting ? <RefreshCw size={13} className="animate-spin" /> : <UploadCloud size={13} />}
            disabled={!preview || isImporting}
            onClick={handleImport}
          >
            {isImporting ? t.modals.importing : t.modals.importExecute}
          </Button>
        </div>
      </div>
    </Modal>
  );
};
