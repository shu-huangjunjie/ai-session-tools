import React from 'react';

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Accessible name; the switch has no visible text of its own. */
  label: string;
  title?: string;
  disabled?: boolean;
}

export const Switch: React.FC<SwitchProps> = ({ checked, onChange, label, title, disabled }) => (
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    title={title}
    disabled={disabled}
    onClick={(e) => {
      // Switches sit inside clickable rows; toggling must not also select the row.
      e.stopPropagation();
      onChange(!checked);
    }}
    className={`relative inline-flex h-5 w-[34px] flex-shrink-0 items-center rounded-full transition-colors duration-150 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/40 disabled:opacity-40 disabled:cursor-not-allowed ${
      checked ? 'bg-blue-600 dark:bg-blue-500' : 'bg-slate-200 dark:bg-slate-700'
    }`}
  >
    <span
      className={`inline-block h-4 w-4 rounded-full bg-white shadow-[0_1px_2px_rgba(15,23,42,0.22)] transition-transform duration-150 ${
        checked ? 'translate-x-[16px]' : 'translate-x-[2px]'
      }`}
    />
  </button>
);
