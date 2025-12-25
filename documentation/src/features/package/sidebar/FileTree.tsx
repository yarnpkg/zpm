import ArrowDownIcon              from '@/assets/svg/arrow-down.svg?react';
import cn                         from '@/utils/cn';
import {formatPackageLink}        from '@/utils/helpers';
import {useState, type ReactNode} from 'preact/compat';

type TreeFolder = Record<string, any>;

export class FileTree {
  tree: TreeFolder = {};

  constructor(files: Array<Array<string>>) {
    for (const file of files) {
      this.pushToTree(file);
    }
  }

  pushToTree(reversedPath: Array<string>) {
    const url = reversedPath[0]; // First item is the file URL
    const pathParts = reversedPath.slice(1); // Get path in correct order

    let folder: TreeFolder = this.tree;

    for (let i = 0; i < pathParts.length; i++) {
      const part = pathParts[i]!;
      const isLast = i === pathParts.length - 1;

      if (isLast) {
        // It's the file name
        folder[part] = url;
      } else {
        // It's a folder
        if (!folder[part])
          folder[part] = {};


        const next = folder[part];
        if (typeof next === `string`)
          throw new Error(`Conflict: '${part}' is a file, can't be a folder`);


        folder = next;
      }
    }
  }
}

interface FileTreeRendererProps {
  node: Record<string, any>; // TreeFolder
  basePath: string;
  name: string;
  version: string;
  currentPath: string;
}

export function FileTreeRenderer({
  node,
  basePath,
  name,
  version,
  currentPath,
}: FileTreeRendererProps) {
  return (
    <>
      {Object.entries(node).map(([key, value]) => {
        const fullPath = `${basePath}${key}`;
        const encodedPath = encodeURIComponent(fullPath);

        if (typeof value === `string`) {
          // It's a file
          return (
            <li key={fullPath}>
              <button
                onClick={() => {
                  window.history.pushState(
                    {},
                    ``,
                    formatPackageLink(name, version, fullPath),
                  );
                  window.dispatchEvent(new PopStateEvent(`popstate`));
                  window.scroll({top: 0, behavior: `smooth`});
                }}
                className={cn(
                  `transition-colors underline-offset-2 font-medium! leading-7 text-left line-clamp-1`,
                  currentPath.endsWith(encodedPath)
                    ? `text-blue-50 font-bold`
                    : `text-white/80 hover:underline hover:text-blue-50`,
                )}
              >
                {key}
              </button>
            </li>
          );
        } else {
          // It's a folder
          return (
            <TreeCollapsable key={fullPath} label={key} id={fullPath}>
              <FileTreeRenderer
                node={value}
                basePath={`${fullPath}/`}
                name={name}
                version={version}
                currentPath={currentPath}
              />
            </TreeCollapsable>
          );
        }
      })}
    </>
  );
}

interface TreeCollapsableProps {
  label: string;
  id: string;
  children: ReactNode;
  initialCollapsed?: boolean;
}

export function TreeCollapsable({
  label,
  id,
  children,
  initialCollapsed = true,
}: TreeCollapsableProps) {
  const [collapsed, setCollapsed] = useState(initialCollapsed);

  return (
    <li>
      <button
        className={`flex items-center text-white w-full justify-between`}
        onClick={() => setCollapsed(prev => !prev)}
        data-open={!collapsed}
        aria-controls={id}
      >
        <span className={`font-medium`}>{label}</span>
        <ArrowDownIcon
          class={`transition-transform w-3 h-3`}
          style={{
            transform: collapsed ? `rotate(0deg)` : `rotate(-180deg)`,
          }}
        />
      </button>

      <ul
        id={id}
        className={`transition-all max-h-0 [[data-collapsed=false]]:!mt-2 [[data-collapsed=false]]:max-h-full opacity-0 [[data-collapsed=false]]:opacity-100 duration-200 overflow-hidden pl-4 flex flex-col gap-y-2 border-l-1 border-white/20`}
        data-collapsed={collapsed}
        aria-expanded={!collapsed}
        inert={collapsed}
      >
        {children}
      </ul>
    </li>
  );
}
