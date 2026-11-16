import React from 'react';

export type BadgeTone = 'gray' | 'blue' | 'red' | 'orange' | 'purple' | 'green';

export const Badge: React.FC<{ tone: BadgeTone; children: React.ReactNode; title?: string }> = ({ tone, children, title }) => {
  const tones = {
    gray: 'bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300 ring-1 ring-inset ring-slate-200/80 dark:ring-slate-700/80',
    blue: 'bg-blue-50 dark:bg-blue-500/15 text-blue-600 dark:text-sky-300 ring-1 ring-inset ring-blue-200/80 dark:ring-blue-400/25',
    red: 'bg-rose-50 dark:bg-rose-500/15 text-rose-600 dark:text-rose-400 ring-1 ring-inset ring-rose-200/80 dark:ring-rose-400/25',
    orange: 'bg-amber-50 dark:bg-amber-500/15 text-amber-700 dark:text-amber-300 ring-1 ring-inset ring-amber-200/80 dark:ring-amber-400/25',
    purple: 'bg-purple-50 dark:bg-purple-500/15 text-purple-600 dark:text-purple-300 ring-1 ring-inset ring-purple-200/80 dark:ring-purple-400/25',
    green: 'bg-emerald-50 dark:bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 ring-1 ring-inset ring-emerald-200/80 dark:ring-emerald-400/25',
  }[tone];
  return (
    <span title={title} className={`inline-flex items-center gap-1 text-[10.5px] font-medium px-2 py-[2px] rounded-full tracking-[0.01em] ${tones}`}>
      {children}
    </span>
  );
};
