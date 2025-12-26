import {type SidebarGroupProps}  from '@/types/sidebar';
import cn                        from '@/utils/cn';
import type {StarlightRouteData} from '@astrojs/starlight/route-data';
import {Fragment, useState}      from 'preact/compat';

import Badge                     from '../Badge';

import SidebarEntry              from './SidebarEntry';

type SidebarEntry = StarlightRouteData[`sidebar`][number];
type SidebarLink = Extract<SidebarEntry, {type: `link`}>;

export default function SidebarGroup({
  badge,
  label,
  collapsed,
  initialCollapsed = false,
  entries,
  className,
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
            ?.filter((entry): entry is SidebarLink => {
              return !entry.label.startsWith(`@yarnpkg/`);
            })
            .map((entry, index) => (
              <li role={`listitem`} key={index}>
                <SidebarEntry
                  {...entry}
                  variant={`sub-link`}
                />
              </li>
            ))}
        </ul>
      </div>
    </div>
  );
}
