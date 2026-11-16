import React from 'react';

export type ButtonVariant = 'primary' | 'accent' | 'secondary' | 'ghost' | 'danger' | 'link';

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: 'sm' | 'md' | 'lg';
  icon?: React.ReactNode;
}

export const Button: React.FC<ButtonProps> = ({
  children,
  variant = 'secondary',
  size = 'md',
  icon,
  className = '',
  disabled,
  // Inside a form, a button without a type submits; Enter would then "click" the
  // first button (often Cancel). Only explicit submit buttons may submit.
  type = 'button',
  ...props
}) => {
  const base =
    'inline-flex items-center justify-center gap-1.5 font-medium rounded-lg whitespace-nowrap transition-all duration-150 active:scale-[0.98] focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/40 select-none disabled:opacity-45 disabled:cursor-not-allowed disabled:active:scale-100';

  const sizeClasses = {
    sm: 'h-[30px] px-2.5 text-xs',
    md: 'h-[34px] px-3 text-xs',
    lg: 'h-9 px-4 text-sm',
  }[size];

  const variantClasses = {
    primary:
      'bg-gradient-to-b from-[#3b82f6] to-[#1d4ed8] hover:from-[#60a5fa] hover:to-[#2563eb] active:from-[#2563eb] active:to-[#1e40af] text-white border border-[#1d4ed8]/90 dark:border-blue-400/35 shadow-[0_2px_6px_rgba(37,99,235,0.28),inset_0_1px_0_rgba(255,255,255,0.2)]',
    accent:
      'bg-blue-50/90 dark:bg-blue-500/12 hover:bg-blue-100/85 dark:hover:bg-blue-500/20 active:bg-blue-200/60 dark:active:bg-blue-500/25 text-blue-700 dark:text-sky-300 border border-blue-200/85 dark:border-blue-400/30 shadow-[0_1px_2px_rgba(37,99,235,0.06)]',
    secondary:
      'bg-white/95 dark:bg-[#192338] hover:bg-slate-50 dark:hover:bg-[#212e47] active:bg-slate-100 dark:active:bg-[#273654] text-slate-700 dark:text-slate-200 border border-slate-200/90 dark:border-slate-700/75 shadow-[0_1px_2px_rgba(15,23,42,0.05),inset_0_1px_0_rgba(255,255,255,0.65)] dark:shadow-[0_1px_2px_rgba(0,0,0,0.25),inset_0_1px_0_rgba(255,255,255,0.04)]',
    ghost:
      'bg-transparent hover:bg-slate-100/90 dark:hover:bg-slate-800/75 active:bg-slate-200/70 dark:active:bg-slate-800 text-slate-500 dark:text-slate-400 hover:text-slate-900 dark:hover:text-slate-100 border border-transparent',
    danger:
      'bg-rose-50/85 dark:bg-rose-500/10 hover:bg-rose-100/85 dark:hover:bg-rose-500/20 active:bg-rose-200/60 text-rose-600 dark:text-rose-400 border border-rose-200/85 dark:border-rose-500/30',
    link:
      'h-auto bg-transparent text-blue-600 dark:text-sky-400 hover:text-blue-700 dark:hover:text-sky-300 p-0 underline border-none',
  }[variant];

  return (
    <button
      type={type}
      className={`${base} ${sizeClasses} ${variantClasses} ${className}`}
      disabled={disabled}
      {...props}
    >
      {icon && <span className="flex-shrink-0">{icon}</span>}
      {children}
    </button>
  );
};
