import type {SidebarEntry} from 'node_modules/@astrojs/starlight/utils/routing/types';

import SidebarEntryElement from './SidebarEntry';

interface Props {
  entries: Array<SidebarEntry>;
  defaultExpandedGroup: number;
}

export default function Sidebar({entries, defaultExpandedGroup}: Props) {
  return entries.map((entry, index) => (
    <SidebarEntryElement
      initialCollapsed={false}
      {...(entry as any)}
      key={index}
    />
  ));
}
