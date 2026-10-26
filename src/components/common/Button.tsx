import React from 'react';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'link';

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
    'inline-flex items-center justify-center gap-1.5 font-medium rounded-lg whitespace-nowrap transition-colors duration-150 focus:outline-none focus-visible:ring-2 focus-visible:ring-[#0071e3]/40 select-none disabled:opacity-40 disabled:cursor-not-allowed';

  const sizeClasses = {
    sm: 'px-2.5 py-1 text-xs',
    md: 'px-3 py-1.5 text-xs',
    lg: 'px-4 py-2 text-sm',
  }[size];

  const variantClasses = {
    primary:
      'bg-[#0071e3] hover:bg-[#0077ed] active:bg-[#0062c4] text-white border border-[#0071e3]',
    secondary:
      'bg-white dark:bg-white/[0.06] hover:bg-[#f5f5f7] dark:hover:bg-white/[0.1] active:bg-black/[0.06] dark:active:bg-white/[0.14] text-[#1d1d1f] dark:text-[#f5f5f7] border border-black/[0.1] dark:border-white/[0.1]',
    ghost:
      'bg-transparent hover:bg-black/[0.05] dark:hover:bg-white/[0.07] active:bg-black/[0.08] dark:active:bg-white/[0.12] text-[#6e6e73] dark:text-[#a1a1a6] hover:text-[#1d1d1f] dark:hover:text-[#f5f5f7] border border-transparent',
    danger:
      'bg-[#ff3b30]/[0.05] dark:bg-[#ff453a]/10 hover:bg-[#ff3b30]/[0.11] dark:hover:bg-[#ff453a]/20 active:bg-[#ff3b30]/[0.16] text-[#d70015] dark:text-[#ff453a] border border-[#ff3b30]/25 dark:border-[#ff453a]/30',
    link:
      'bg-transparent text-[#0071e3] dark:text-[#2997ff] hover:text-[#0077ed] dark:hover:text-[#64b5ff] p-0 underline border-none active:scale-100',
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
