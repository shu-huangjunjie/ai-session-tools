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
  align?: 'left' | 'right';
}

export const DropdownMenu: React.FC<DropdownMenuProps> = ({
  items,
  label,
  title,
  icon,
  trailing,
  variant = 'secondary',
  align = 'right',
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
    <div ref={ref} className="relative">
      <Button
        variant={variant}
        icon={icon}
        title={title}
        aria-label={label ? undefined : title}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className={label ? '' : 'px-2'}
      >
        {label}
        {trailing}
      </Button>
      {open && (
        <div
          role="menu"
          className={`absolute top-full mt-1.5 ${align === 'right' ? 'right-0' : 'left-0'} z-50 min-w-[180px] p-1.5 rounded-xl bg-white dark:bg-[#1c1c1e] border border-black/[0.08] dark:border-white/[0.12] shadow-[0_8px_24px_rgba(0,0,0,0.12)] text-xs text-[#1d1d1f] dark:text-[#ededf0] animate-in fade-in zoom-in-95 duration-100`}
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
              className="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-left hover:bg-black/[0.05] dark:hover:bg-white/[0.08] disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            >
              <span className="w-3.5 flex-shrink-0 flex items-center justify-center text-[#6e6e73] dark:text-[#a1a1a6]">
                {item.checked === undefined ? item.icon : item.checked ? <Check size={13} /> : null}
              </span>
              <span className="flex-1 whitespace-nowrap">{item.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
};
