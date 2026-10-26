import React from 'react';

export interface SegmentedItem<K extends string> {
  key: K;
  label: React.ReactNode;
  count?: React.ReactNode;
  /** Colour dot that marks a client. */
  dot?: string;
  title?: string;
  /** Accessible name when the label is not plain text. */
  ariaLabel?: string;
}

interface SegmentedProps<K extends string> {
  items: SegmentedItem<K>[];
  value: K | undefined;
  onChange: (key: K) => void;
  /** Extra controls after the tabs, such as an add button. */
  children?: React.ReactNode;
}

export function Segmented<K extends string>({ items, value, onChange, children }: SegmentedProps<K>) {
  return (
    <div
      role="tablist"
      className="flex items-center gap-0.5 p-[3px] rounded-[9px] bg-black/[0.05] dark:bg-white/[0.07] min-w-0 overflow-x-auto no-scrollbar"
    >
      {items.map((item) => {
        const active = item.key === value;
        return (
          <button
            key={item.key}
            type="button"
            role="tab"
            aria-selected={active}
            aria-label={item.ariaLabel}
            title={item.title}
            onClick={() => onChange(item.key)}
            className={`flex items-center gap-1.5 px-3 py-[5px] rounded-[7px] text-xs whitespace-nowrap transition-all duration-150 ${
              active
                ? 'bg-white dark:bg-[#2c2c2e] text-[#1d1d1f] dark:text-white font-semibold shadow-[0_1px_2px_rgba(0,0,0,0.1)]'
                : 'text-[#6e6e73] dark:text-[#a1a1a6] hover:text-[#1d1d1f] dark:hover:text-white'
            }`}
          >
            {item.dot && <span className="w-[7px] h-[7px] rounded-full flex-shrink-0" style={{ background: item.dot }} />}
            <span className="flex items-center gap-1 max-w-[200px] truncate">{item.label}</span>
            {item.count !== undefined && (
              <span className="text-[11px] font-mono font-normal text-[#86868b]">{item.count}</span>
            )}
          </button>
        );
      })}
      {children}
    </div>
  );
}
