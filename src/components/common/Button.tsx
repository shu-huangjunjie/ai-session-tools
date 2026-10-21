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
    'inline-flex items-center justify-center gap-1.5 font-medium rounded-full transition-all duration-200 ease-out focus:outline-none select-none active:scale-[0.97] disabled:opacity-40 disabled:cursor-not-allowed disabled:active:scale-100';

  const sizeClasses = {
    sm: 'px-3 py-1 text-xs',
    md: 'px-3.5 py-1.5 text-xs',
    lg: 'px-5 py-2 text-sm',
  }[size];

  const variantClasses = {
    primary:
      'bg-[#0071e3] hover:bg-[#0077ed] active:bg-[#0062c4] text-white shadow-[0_2px_10px_rgba(0,113,227,0.3)] border border-transparent dark:border-white/10',
    secondary:
      'bg-black/[0.05] dark:bg-white/[0.08] hover:bg-black/[0.08] dark:hover:bg-white/[0.13] active:bg-black/[0.12] dark:active:bg-white/[0.18] text-[#1d1d1f] dark:text-[#f5f5f7] border border-black/[0.08] dark:border-white/[0.08] backdrop-blur-md shadow-sm',
    ghost:
      'bg-transparent hover:bg-black/[0.05] dark:hover:bg-white/[0.07] active:bg-black/[0.08] dark:active:bg-white/[0.12] text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-[#f5f5f7] border border-transparent',
    danger:
      'bg-[#ff3b30]/10 dark:bg-[#ff453a]/15 hover:bg-[#ff3b30]/20 dark:hover:bg-[#ff453a]/25 active:bg-[#ff3b30]/30 text-[#ff3b30] dark:text-[#ff453a] border border-[#ff3b30]/20 dark:border-[#ff453a]/30 shadow-sm',
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
