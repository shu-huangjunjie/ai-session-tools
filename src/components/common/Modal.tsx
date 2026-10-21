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
      className="fixed inset-0 z-50 flex items-center justify-center p-4 md:p-6 bg-black/40 dark:bg-black/80 backdrop-blur-2xl animate-in fade-in duration-200 select-none"
      onClick={onClose}
    >
      <div
        className={`w-full ${widthClass} bg-white/95 dark:bg-[#161618]/95 border border-black/[0.08] dark:border-white/[0.12] rounded-3xl shadow-[0_20px_50px_rgba(0,0,0,0.15)] dark:shadow-[0_24px_64px_rgba(0,0,0,0.8)] flex flex-col max-h-[88vh] overflow-hidden animate-in zoom-in-95 duration-200`}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Apple Modal Header */}
        <div className="flex items-center justify-between px-6 py-4.5 border-b border-black/[0.06] dark:border-white/[0.08] bg-black/[0.01] dark:bg-white/[0.02]">
          <div>
            <h3 className="text-base font-semibold tracking-tight text-[#1d1d1f] dark:text-white">
              {title}
            </h3>
            {subtitle && (
              <p className="text-xs text-[#86868b] mt-0.5 leading-relaxed">{subtitle}</p>
            )}
          </div>
          <button
            onClick={onClose}
            className="w-7 h-7 rounded-full bg-black/[0.05] dark:bg-white/[0.08] hover:bg-black/[0.1] dark:hover:bg-white/[0.16] text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white flex items-center justify-center transition-all"
            title="关闭 (Esc)"
          >
            <X size={14} />
          </button>
        </div>

        {/* Content */}
        <div className="p-6 overflow-y-auto flex-1 text-[#1d1d1f] dark:text-[#ededf0]">{children}</div>
      </div>
    </div>
  );
};
