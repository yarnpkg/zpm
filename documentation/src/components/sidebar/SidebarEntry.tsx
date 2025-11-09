import {type SidebarEntryProps} from 'src/types/sidebar';

import SidebarGroup             from './SidebarGroup';
import SidebarLink              from './SidebarLink';

export default function SidebarEntry({variant = `link`, ...props}: SidebarEntryProps & {variant?: `link` | `sub-link`}) {
  return props.type === `link` ? (
    <div className={`size-full`}>
      <SidebarLink {...props} variant={variant} />
    </div>
  ) : (
    <div>
      <SidebarGroup {...props} variant={variant} />
    </div>
  );
}
