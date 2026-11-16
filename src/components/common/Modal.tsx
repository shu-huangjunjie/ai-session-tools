import React, { useEffect } from 'react';
import { X } from 'lucide-react';

interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  subtitle?: string;
  children: React.ReactNode;
  widthClass?: string;
}

export const Modal: React.FC<ModalProps> = ({
  isOpen,
  onClose,
  title,
  subtitle,
  children,
  widthClass = 'max-w-2xl',
}) => {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && isOpen) {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4 md:p-6 bg-slate-900/45 dark:bg-black/70 backdrop-blur-[3px] animate-in fade-in duration-150 select-none"
      onClick={onClose}
    >
      <div
        className={`w-full ${widthClass} bg-white dark:bg-[#141d2f] border border-slate-200 dark:border-slate-700 rounded-3xl shadow-[0_24px_64px_rgba(15,23,42,0.28)] dark:shadow-[0_24px_72px_rgba(0,0,0,0.7)] flex flex-col max-h-[88vh] overflow-hidden animate-in zoom-in-95 duration-150`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-6 py-4 border-b border-slate-200 dark:border-slate-800 bg-slate-50 dark:bg-[#182237]">
          <div>
            <h3 className="text-base font-semibold tracking-tight text-slate-900 dark:text-white">
              {title}
            </h3>
            {subtitle && (
              <p className="text-xs text-slate-600 dark:text-slate-300 mt-0.5 leading-relaxed">{subtitle}</p>
            )}
          </div>
          <button
            onClick={onClose}
            className="w-7 h-7 rounded-full bg-slate-200 dark:bg-slate-800 hover:bg-slate-300 dark:hover:bg-slate-700 text-slate-600 dark:text-slate-300 hover:text-slate-900 dark:hover:text-white flex items-center justify-center transition-all"
            title="关闭 (Esc)"
          >
            <X size={14} />
          </button>
        </div>

        <div className="p-6 overflow-y-auto flex-1 bg-white dark:bg-[#141d2f] text-slate-800 dark:text-slate-200">{children}</div>
      </div>
    </div>
  );
};
