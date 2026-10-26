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
  <header className="h-[60px] flex-shrink-0 flex items-center gap-4 px-5 bg-white dark:bg-[#161618] border-b border-black/[0.06] dark:border-white/[0.08] select-none z-20 transition-colors">
    <h1 className="text-[17px] font-semibold tracking-tight text-[#1d1d1f] dark:text-white whitespace-nowrap">{title}</h1>
    {tabs}
    <div className="flex-1" />
    <div className="flex items-center gap-2 flex-shrink-0">{children}</div>
  </header>
);
