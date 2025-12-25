import type {StarlightRouteData} from '@astrojs/starlight/route-data';

import SidebarEntryElement       from './SidebarEntry';

type SidebarEntry = StarlightRouteData[`sidebar`][number];

interface Props {
  entries: Array<SidebarEntry>;
  defaultExpandedGroup: number;
}

export default function Sidebar({entries, defaultExpandedGroup}: Props) {
  return entries.map((entry, index) => (
    <SidebarEntryElement key={index} {...entry}/>
  ));
}
