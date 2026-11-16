import React from 'react';
import { CheckCircle2, AlertTriangle, XCircle, Info } from 'lucide-react';

export type ToastType = 'success' | 'warning' | 'error' | 'info';

export interface ToastMessage {
  id: string;
  type: ToastType;
  message: string;
}

interface ToastContainerProps {
  toasts: ToastMessage[];
  onDismiss: (id: string) => void;
}

export const ToastContainer: React.FC<ToastContainerProps> = ({ toasts, onDismiss }) => {
  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-6 right-6 z-50 flex flex-col gap-2.5 max-w-sm pointer-events-none select-none">
      {toasts.map((t) => {
        const iconColor = {
          success: 'text-emerald-600 dark:text-emerald-400',
          warning: 'text-amber-500 dark:text-amber-400',
          error: 'text-rose-600 dark:text-rose-400',
          info: 'text-blue-600 dark:text-sky-400',
        }[t.type];

        const Icon = {
          success: CheckCircle2,
          warning: AlertTriangle,
          error: XCircle,
          info: Info,
        }[t.type];

        return (
          <div
            key={t.id}
            onClick={() => onDismiss(t.id)}
            className="pointer-events-auto flex items-center gap-2.5 px-4 py-2.5 rounded-2xl border border-slate-200/95 dark:border-slate-700/85 bg-white/96 dark:bg-[#1a2436]/96 backdrop-blur-xl shadow-[0_10px_28px_rgba(15,23,42,0.12)] dark:shadow-[0_12px_36px_rgba(0,0,0,0.5)] text-xs font-medium text-slate-800 dark:text-slate-100 cursor-pointer transition-all hover:scale-[1.01] active:scale-[0.98] animate-in slide-in-from-bottom-3 duration-200"
          >
            <Icon size={15} className={`flex-shrink-0 ${iconColor}`} />
            <span className="flex-1 leading-snug tracking-tight">{t.message}</span>
          </div>
        );
      })}
    </div>
  );
};
