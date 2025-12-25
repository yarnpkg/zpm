import type {StarlightRouteData} from '@astrojs/starlight/route-data';
import type {HTMLAttributes}     from 'preact/compat';

type SidebarEntry = StarlightRouteData[`sidebar`][number];

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: `tip` | `note` | `success` | `danger` | `caution` | `package` | `author` | `default`;
  text: string;
  className?: string;
}

export type SidebarLinkProps = Extract<SidebarEntry, {type: `link`}> & {className?: string, variant?: `link` | `sub-link`};
export type SidebarGroupProps = Extract<SidebarEntry, {type: `group`}> & {className?: string, initialCollapsed?: boolean};

export type SidebarEntryProps = SidebarLinkProps | SidebarGroupProps;
