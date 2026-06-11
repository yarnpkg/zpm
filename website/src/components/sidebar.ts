export type SidebarLink = {
  label: string;
  href: string;
  active?: boolean;
  sub?: boolean;
  mono?: boolean;
  section?: boolean;
};

export type SidebarSubtitle = {
  subtitle: string;
};

export type SidebarItem = SidebarLink | SidebarSubtitle;

export interface SidebarGroup {
  title: string;
  items: Array<SidebarItem>;
}

const metaGlob = import.meta.glob<string>(`../docs/**/_meta.{yml,yaml}`, {eager: true, query: `?raw`, import: `default`});
const docGlob = import.meta.glob<string>(`../docs/**/*.md`, {eager: true, query: `?raw`, import: `default`});

const metaLookup = new Map<string, {label: string, order: number}>();

for (const [filePath, content] of Object.entries(metaGlob)) {
  const relDir = filePath
    .replace(/^\.\.\/docs\//, ``)
    .replace(/\/_meta\.(yml|yaml)$/, ``);
  const label = content.match(/^label:\s*(.+)$/m)?.[1]?.trim();
  const order = parseInt(content.match(/^order:\s*(\d+)$/m)?.[1] ?? `99`, 10);
  metaLookup.set(relDir, {label: label ?? relDir, order});
}

const slugToDir = new Map<string, string>();

for (const [filePath, content] of Object.entries(docGlob)) {
  const slug = content.match(/^slug:\s*(.+)$/m)?.[1]?.trim();
  if (slug) {
    const relPath = filePath.replace(/^\.\.\/docs\//, ``);
    const lastSlash = relPath.lastIndexOf(`/`);
    slugToDir.set(slug, lastSlash >= 0 ? relPath.substring(0, lastSlash) : `.`);
  }
}

export function formatLabel(dirName: string): string {
  return dirName
    .split(`-`)
    .map(w => w[0].toUpperCase() + w.slice(1))
    .join(` `);
}

export function getDirForSlug(slug: string): string | undefined {
  return slugToDir.get(slug);
}

export function getMetaForDir(dir: string): {label: string, order: number} | undefined {
  return metaLookup.get(dir);
}

export function getGroupLabelForSlug(slug: string): string | undefined {
  const dir = slugToDir.get(slug);
  if (!dir) return undefined;
  const meta = metaLookup.get(dir);
  return meta?.label ?? formatLabel(dir.split(`/`).pop()!);
}

export function buildSidebarGroups(
  allDocs: Array<{data: {slug: string, title: string, sidebar?: {order?: number, hidden?: boolean}, sidebar_position?: number}}>,
  section: string,
  activePage: string,
): Array<SidebarGroup> {
  const docs = allDocs.filter(doc => {
    const dir = getDirForSlug(doc.data.slug);
    if (!dir?.startsWith(section)) return false;
    if (doc.data.sidebar?.hidden) return false;

    return true;
  });

  const groupMap = new Map<string, {label: string, sortKey: number, docs: typeof docs}>();

  for (const doc of docs) {
    const fsDir = getDirForSlug(doc.data.slug) ?? `.`;

    if (!groupMap.has(fsDir)) {
      const meta = getMetaForDir(fsDir);
      groupMap.set(fsDir, {
        label: meta?.label ?? formatLabel(fsDir.split(`/`).pop()!),
        sortKey: meta?.order ?? 99,
        docs: [],
      });
    }

    groupMap.get(fsDir)!.docs.push(doc);
  }

  return [...groupMap.values()]
    .sort((a, b) => a.sortKey - b.sortKey)
    .map(({label, docs: groupDocs}) => ({
      title: label,
      items: groupDocs
        .sort((a, b) => {
          const orderA = a.data.sidebar?.order ?? a.data.sidebar_position ?? 99;
          const orderB = b.data.sidebar?.order ?? b.data.sidebar_position ?? 99;
          return orderA - orderB;
        })
        .map(doc => ({
          label: doc.data.title,
          href: `/${doc.data.slug}/`,
          active: doc.data.slug === activePage,
        })),
    }));
}
