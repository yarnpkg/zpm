import cn                      from '@/utils/cn';
import {type SidebarLinkProps} from 'src/types/sidebar';

import Badge                   from '../Badge';

export default function SidebarLink({
  badge,
  label,
  variant,
  isCurrent,
  className,
  attrs,
  type,
  initialCollapsed,
  ...props
}: SidebarLinkProps) {
  if (`entries` in props && (props.entries as Array<SidebarLinkProps>).some(entry => entry.isCurrent))
    isCurrent = true;

  return (
    <a {...props} aria-label={label} {...attrs} className={`size-full`}>
      <div
        className={cn(
          `leading-[1.4em] text-start size-full transition-all font-medium hover:text-blue-50 font-montserrat text-white/80`,
          variant === `link`
            ? `text-xl hover:bg-white/3 rounded-lg py-3 px-4`
            : `text-base`,
          isCurrent && variant === `link` && `text-blue-50 bg-white/3`,
          isCurrent && variant === `sub-link` && `text-blue-50`,
          badge && `flex items-center justify-between`,
          className,
        )}
      >
        <span>{label}</span>
        {badge && <Badge {...badge} />}
      </div>
    </a>
  );
}
