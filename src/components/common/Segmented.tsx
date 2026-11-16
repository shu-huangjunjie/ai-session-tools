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
      className="flex items-center gap-0.5 p-[3px] rounded-xl bg-slate-200/60 dark:bg-[#0b101b]/85 border border-slate-200/80 dark:border-slate-800/90 shadow-[inset_0_1px_2px_rgba(15,23,42,0.04)] min-w-0 overflow-x-auto no-scrollbar"
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
            className={`flex items-center gap-1.5 px-3 py-[5px] rounded-[9px] text-xs whitespace-nowrap transition-all duration-150 ${
              active
                ? 'bg-white dark:bg-[#1e2940] text-slate-900 dark:text-white font-semibold shadow-[0_1px_3px_rgba(15,23,42,0.10)] ring-1 ring-slate-200/80 dark:ring-slate-700/80'
                : 'text-slate-500 dark:text-slate-400 hover:text-slate-800 dark:hover:text-slate-200 hover:bg-white/60 dark:hover:bg-white/[0.04]'
            }`}
          >
            {item.dot && (
              <span
                className="w-[7px] h-[7px] rounded-full flex-shrink-0"
                style={{
                  background: item.dot,
                  boxShadow: active ? `0 0 0 2px ${item.dot}26, 0 0 6px ${item.dot}66` : undefined,
                }}
              />
            )}
            <span className="flex items-center gap-1 max-w-[200px] truncate">{item.label}</span>
            {item.count !== undefined && (
              <span
                className={`text-[10.5px] font-mono px-1.5 py-px rounded-md ${
                  active
                    ? 'bg-blue-50 dark:bg-blue-500/15 text-blue-600 dark:text-sky-300 font-medium'
                    : 'bg-slate-200/75 dark:bg-slate-800/85 text-slate-500 dark:text-slate-400 font-normal'
                }`}
              >
                {item.count}
              </span>
            )}
          </button>
        );
      })}
      {children}
    </div>
  );
}
