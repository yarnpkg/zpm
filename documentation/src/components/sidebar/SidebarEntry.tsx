import {type SidebarEntryProps} from '@/types/sidebar';

import SidebarGroup             from './SidebarGroup';
import SidebarLink              from './SidebarLink';

export default function SidebarEntry(props: SidebarEntryProps) {
  return props.type === `link` ? (
    <div className={`size-full`}>
      <SidebarLink {...props} variant={props.variant}/>
    </div>
  ) : (
    <div>
      <SidebarGroup {...props}/>
    </div>
  );
}
