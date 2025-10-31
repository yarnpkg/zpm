import cn                                    from '@/utils/cn';
import type {SidebarLink as SidebarLinkType} from 'node_modules/@astrojs/starlight/utils/routing/types';
import {Fragment, useState}                  from 'preact/compat';
import {type SidebarGroupProps}              from 'src/types/sidebar';

import Badge                                 from '../Badge';

import SidebarLink                           from './SidebarLink';

export default function SidebarGroup({
  badge,
  label,
  collapsed,
  initialCollapsed,
  entries,
  className,
  variant,
  type: _,
  ...props
}: SidebarGroupProps) {
  const [collapseState, setCollapseState] = useState<boolean>(initialCollapsed);

  const classNames = cn(`text-lg leading-[1.4] font-medium`, className);

  let indexHref: string | undefined = undefined;
  if (!label) {
    const entry = entries.toReversed().pop(); // Get the first entry safely.
    if (entry && entry.label.startsWith(`@yarnpkg/`) && entry.type === `link`) {
      label = entry.label;
      indexHref = entry.href;
    }
  }

  const Wrapper = indexHref ? `a` : Fragment;
  const Element = indexHref ? `div` : `button`;

  return (
    <div class={classNames} {...props}>
      <Wrapper href={indexHref && indexHref} class={indexHref && `size-full`}>
        <Element
          class={cn(
            `px-4 py-3 group text-start size-full rounded-lg font-montserrat text-white`,
            badge && `flex items-center justify-between`,
          )}
        >
          <span>{label}</span> {badge && <Badge {...badge} />}
        </Element>
      </Wrapper>
      <div
        id={
          indexHref ? undefined : `sidebar-group-${label?.replace(/\s+/g, `-`)}`
        }
        data-collapsed={indexHref ? undefined : collapseState}
        inert={indexHref ? undefined : collapseState}
        class={`px-4 py-2 font-montserrat`}
      >
        <ul
          role={`list`}
          className={`border-l pl-4 border-white/10 flex flex-col gap-y-3 font-montserrat`}
        >
          {entries
            .filter((entry): entry is SidebarLinkType => {
              return !entry.label.startsWith(`@yarnpkg/`);
            })
            .map(({type: _, attrs, ...entry}, index) => {
              if (!(`entries` in entry)) {
                return (
                  <li role={`listitem`} key={index}>
                    <SidebarLink
                      {...entry}
                      {...(attrs as any)}
                      variant={`sub-link`}
                    />
                  </li>
                );
              } else {
                return (
                  <li role={`listitem`} key={index}>
                    <SidebarLink
                      {...entry}
                      {...(attrs as any)}
                      href={`/${entry.label.toLowerCase()}`}
                      variant={`sub-link`}
                    />
                  </li>
                );
              }
            })}
        </ul>
      </div>
    </div>
  );
}
