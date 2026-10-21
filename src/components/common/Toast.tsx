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
          success: 'text-emerald-400',
          warning: 'text-amber-400',
          error: 'text-[#ff453a]',
          info: 'text-[#2997ff]',
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
            className="pointer-events-auto flex items-center gap-2.5 px-4 py-2.5 rounded-full border border-white/[0.12] bg-[#1c1c1e]/90 backdrop-blur-2xl shadow-[0_8px_32px_rgba(0,0,0,0.6)] text-xs font-medium text-[#f5f5f7] cursor-pointer transition-all hover:scale-[1.02] active:scale-[0.98] animate-in slide-in-from-bottom-3 duration-200"
          >
            <Icon size={15} className={`flex-shrink-0 ${iconColor}`} />
            <span className="flex-1 leading-snug tracking-tight">{t.message}</span>
          </div>
        );
      })}
    </div>
  );
};
