import React from 'react';

export type BadgeTone = 'gray' | 'blue' | 'red' | 'orange' | 'purple' | 'green';

export const Badge: React.FC<{ tone: BadgeTone; children: React.ReactNode; title?: string }> = ({ tone, children, title }) => {
  const tones = {
    gray: 'bg-black/[0.05] dark:bg-white/[0.08] text-[#6e6e73] dark:text-[#a1a1a6]',
    blue: 'bg-[#0071e3]/10 text-[#0071e3] dark:text-[#2997ff]',
    red: 'bg-[#ff3b30]/10 text-[#d70015] dark:text-[#ff453a]',
    orange: 'bg-[#ff9f0a]/15 text-[#c93400] dark:text-[#ff9f0a]',
    purple: 'bg-[#af52de]/10 text-[#af52de] dark:text-[#bf5af2]',
    green: 'bg-[#34c759]/12 text-[#248a3d] dark:text-[#30d158]',
  }[tone];
  return (
    <span title={title} className={`inline-flex items-center gap-1 text-[10px] font-medium px-1.5 py-0.5 rounded-full ${tones}`}>
      {children}
    </span>
  );
};
