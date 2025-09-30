import ArrowDownIcon              from '/src/assets/svg/arrow-down.svg?react';
import cn                         from '@/utils/cn';
import {useState, type ReactNode} from 'preact/compat';

interface CollapsableProps {
  initialCollapsed?: boolean;
  label: string;
  children: ReactNode;
  id: string;
  expandFull?: boolean;
}

export default function Collapsable({
  initialCollapsed,
  label,
  children,
  id,
  expandFull,
}: CollapsableProps) {
  const [collapsed, setCollapsed] = useState<boolean>(initialCollapsed ?? true);

  return (
    <div>
      <button
        className={`w-full transition-colors text-center flex justify-between items-center max-lg:bg-white/3 p-4 rounded-lg`}
        onClick={() => setCollapsed(state => !state)}
        data-open={!collapsed}
        aria-controls={id}
      >
        <span className={`font-medium text-xl text-white leading-7`}>
          {label}
        </span>

        <ArrowDownIcon
          class={`transition-all [[data-open=true]]:-rotate-180`}
          data-open={!collapsed}
        />
      </button>
      <ul
        className={cn(
          `flex flex-col gap-y-4 scroll-smooth underline-offset-2 duration-300 opacity-0 max-h-0 !m-0 transition-all [[data-collapsed=false]]:opacity-100 [[data-collapsed=false]]:py-2 overflow-y-scroll px-8!`,
          expandFull
            ? `[[data-collapsed=false]]:max-h-full`
            : `[[data-collapsed=false]]:max-h-100`,
        )}
        data-collapsed={collapsed}
        aria-expanded={!collapsed}
        inert={collapsed}
        id={id}
        style={{
          scrollbarWidth: `thin`,
          scrollbarColor: `#ffffff66 transparent`,
        }}
      >
        {children}
      </ul>
    </div>
  );
}
