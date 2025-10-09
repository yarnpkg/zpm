import cn                from '@/utils/cn';
import {type BadgeProps} from 'src/types/sidebar';

const variants = {
  note: `bg-blue-950 border-blue-500`,
  tip: `bg-fuchsia-950 border-fuchsia-500`,
  success: `bg-green-900 border-green-500`,
  danger: `bg-rose-950 border-rose-400`,
  caution: `bg-yellow-800 border-yellow-400`,
  package: `bg-purple-800/5 border-white/10 text-purple-800`,
  author: `bg-blue-600/5 border-white/10 text-blue-50`,
}; // Can be updated later

export default function Badge({variant = `default`, className, text, ...props}: BadgeProps) {
  return (
    <span
      role={`status`}
      class={cn(
        `py-0.5 rounded-sm px-1.5 border text-xs text-white font-[var(--sl-font-system-mono)]`,
        variants[!variant || variant === `default` ? `note` : variant],
        className,
      )}
      {...props}
    >
      {text}
    </span>
  );
}
