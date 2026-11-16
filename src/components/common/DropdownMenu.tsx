import React, { useEffect, useRef, useState } from 'react';
import { Check } from 'lucide-react';
import { Button, ButtonVariant } from './Button';

export interface DropdownItem {
  key: string;
  label: string;
  icon?: React.ReactNode;
  onSelect: () => void;
  /** Shown as a checkable item when set. */
  checked?: boolean;
  disabled?: boolean;
}

interface DropdownMenuProps {
  items: DropdownItem[];
  /** Visible text of the trigger; omit for an icon-only trigger. */
  label?: string;
  /** Accessible name and tooltip of the trigger. */
  title: string;
  icon?: React.ReactNode;
  /** Shown after the label, such as a chevron. */
  trailing?: React.ReactNode;
  variant?: ButtonVariant;
  size?: 'sm' | 'md' | 'lg';
  align?: 'left' | 'right';
  triggerClassName?: string;
}

export const DropdownMenu: React.FC<DropdownMenuProps> = ({
  items,
  label,
  title,
  icon,
  trailing,
  variant = 'secondary',
  size = 'md',
  align = 'right',
  triggerClassName = '',
}) => {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent && e.key !== 'Escape') return;
      if (e.type === 'mousedown' && ref.current?.contains(e.target as Node)) return;
      setOpen(false);
    };
    window.addEventListener('mousedown', close);
    window.addEventListener('keydown', close);
    window.addEventListener('blur', close);
    return () => {
      window.removeEventListener('mousedown', close);
      window.removeEventListener('keydown', close);
      window.removeEventListener('blur', close);
    };
  }, [open]);

  return (
    <div ref={ref} className="relative inline-flex">
      <Button
        variant={variant}
        size={size}
        icon={icon}
        title={title}
        aria-label={label ? undefined : title}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className={`${label ? '' : 'px-2'} ${triggerClassName}`.trim()}
      >
        {label}
        {trailing}
      </Button>
      {open && (
        <div
          role="menu"
          className={`absolute top-full mt-1.5 ${align === 'right' ? 'right-0' : 'left-0'} z-50 min-w-[200px] p-1.5 rounded-xl bg-white dark:bg-[#1a2336] border border-slate-200 dark:border-slate-700 shadow-[0_16px_40px_rgba(15,23,42,0.18)] dark:shadow-[0_16px_40px_rgba(0,0,0,0.6)] text-[12.5px] text-slate-800 dark:text-slate-100 animate-in fade-in zoom-in-95 duration-100`}
        >
          {items.map((item) => (
            <button
              key={item.key}
              type="button"
              role={item.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
              aria-checked={item.checked}
              disabled={item.disabled}
              onClick={() => {
                setOpen(false);
                item.onSelect();
              }}
              className="w-full flex items-center gap-2 px-2.5 py-2 rounded-lg text-left font-medium hover:bg-slate-100 dark:hover:bg-slate-700/60 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            >
              <span className="w-3.5 flex-shrink-0 flex items-center justify-center text-slate-600 dark:text-slate-300">
                {item.checked === undefined ? item.icon : item.checked ? <Check size={13} className="text-blue-600 dark:text-sky-400" /> : null}
              </span>
              <span className="flex-1 whitespace-nowrap">{item.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
