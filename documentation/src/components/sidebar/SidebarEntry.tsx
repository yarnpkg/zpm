import {type SidebarEntryProps} from 'src/types/sidebar';

import SidebarGroup             from './SidebarGroup';
import SidebarLink              from './SidebarLink';

export default function SidebarEntry(props: SidebarEntryProps) {
  return props.type === `link` ? (
    <div className={`size-full`}>
      <SidebarLink {...props} variant={`link`} />
    </div>
  ) : (
    <div>
      <SidebarGroup {...props} />
    </div>
  );
}
