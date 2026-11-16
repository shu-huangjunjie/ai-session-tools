import React from 'react';

interface PageHeaderProps {
  title: string;
  /** Client or location tabs next to the title. */
  tabs?: React.ReactNode;
  /** Page actions on the right. */
  children?: React.ReactNode;
}

/** The bar at the top of every page: title, tabs, then the page's actions. */
export const PageHeader: React.FC<PageHeaderProps> = ({ title, tabs, children }) => (
  <header className="h-[58px] flex-shrink-0 flex items-center gap-3.5 px-5 bg-white/82 dark:bg-[#121a2a]/82 backdrop-blur-xl border-b border-slate-200/75 dark:border-slate-800/80 shadow-[0_1px_0_rgba(15,23,42,0.03)] select-none z-20 transition-colors">
    <h1 className="text-[16px] font-semibold tracking-tight text-slate-900 dark:text-white whitespace-nowrap">{title}</h1>
    {tabs}
    <div className="flex-1" />
    <div className="flex items-center gap-2 flex-shrink-0">{children}</div>
  </header>
);
