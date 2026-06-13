import type {FileEntry, PmTab, ParsedUrl, RegistryData, Tab, TreeNode} from './types';

// ── URL Parsing ──

const TAB_NAMES = new Set<string>([`versions`, `files`, `file`, `audit`]);

export function parseSplat(splat: string): ParsedUrl {
  const parts = splat.split(`/`).map(decodeURIComponent).filter(Boolean);
  if (!parts.length)
    return {name: ``};

  let idx = 0;

  let name: string;
  if (parts[0].startsWith(`@`) && parts.length >= 2) {
    name = `${parts[0]}/${parts[1]}`;
    idx = 2;
  } else {
    name = parts[0];
    idx = 1;
  }

  let version: string | undefined;
  let compareVersion: string | undefined;
  let tab: Tab | undefined;
  let filePath: string | undefined;

  if (idx < parts.length && !TAB_NAMES.has(parts[idx])) {
    const segment = parts[idx];
    const dotdot = segment.indexOf(`..`);
    if (dotdot !== -1) {
      version = segment.slice(0, dotdot);
      compareVersion = segment.slice(dotdot + 2);
    } else {
      version = segment;
    }
    idx++;
  }

  if (idx < parts.length && TAB_NAMES.has(parts[idx])) {
    const segment = parts[idx];
    idx++;
    if (segment === `file`) {
      tab = `files`;
      filePath = parts.slice(idx).join(`/`) || undefined;
    } else {
      tab = segment as Tab;
    }
  }

  return {name, version, compareVersion, tab, filePath};
}

export function packagePath(name: string, version?: string, tab?: Tab, filePath?: string, compareVersion?: string): string {
  let p = `/package/${name}`;
  if (version) {
    p += `/${version}`;
    if (compareVersion) {
      p += `..${compareVersion}`;
    }
  }
  if (tab && tab !== `readme`) {
    if (tab === `files` && filePath) {
      p += `/file/${filePath}`;
    } else {
      p += `/${tab}`;
    }
  }
  return p;
}

// ── Formatting Helpers ──

export function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n >= 10_000 ? 0 : 1)}k`;
  return n.toLocaleString();
}

export function formatNumberFull(n: number): string {
  return n.toLocaleString();
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000) return `${(bytes / 1_000_000).toFixed(1)} MB`;
  if (bytes >= 1_000) return `${(bytes / 1_000).toFixed(1)} kB`;
  return `${bytes} B`;
}

export function formatDate(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleDateString(`en-US`, {month: `short`, day: `numeric`, year: `numeric`});
}

export function formatDateShort(dateStr: string): string {
  const d = new Date(dateStr);
  const month = d.toLocaleDateString(`en-US`, {month: `short`});
  const day = d.getDate();
  const year = String(d.getFullYear()).slice(2);
  return `${month} ${day} '${year}`;
}

export function timeAgo(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime();

  const days = Math.floor(diff / 86400000);
  if (days < 1) return `today`;
  if (days === 1) return `1d`;
  if (days < 30) return `${days}d`;

  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo`;

  const years = Math.floor(months / 12);
  return `${years}y`;
}

export function getLicense(license: RegistryData[`license`]): string {
  if (!license) return `Unknown`;
  if (typeof license === `string`) return license;
  return license.type || `Unknown`;
}

export function getRepoUrl(repo: RegistryData[`repository`]): string | null {
  if (!repo)
    return null;

  let url = typeof repo === `string` ? repo : repo.url;
  if (!url)
    return null;

  url = url
    .replace(/^git\+/, ``)
    .replace(/\.git$/, ``)
    .replace(/^git:\/\//, `https://`)
    .replace(/^ssh:\/\/git@/, `https://`);

  return url;
}

export function getBugsUrl(bugs: RegistryData[`bugs`]): string | null {
  if (!bugs) return null;
  return typeof bugs === `string` ? bugs : bugs.url || null;
}

// ── Markdown Renderer ──

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, `&amp;`)
    .replace(/</g, `&lt;`)
    .replace(/>/g, `&gt;`)
    .replace(/"/g, `&quot;`);
}

function isSafeUrl(url: string): boolean {
  const decoded = url.replace(/&amp;/g, `&`);
  if (decoded.startsWith(`/`) || decoded.startsWith(`#`)) return true;
  try {
    const parsed = new URL(decoded);
    return [`https:`, `http:`, `mailto:`].includes(parsed.protocol);
  } catch {
    return false;
  }
}

export function renderMarkdown(md: string): string {
  if (!md)
    return ``;

  const codeBlocks: Array<string> = [];

  let html = md.replace(/```(\w*)\n([\s\S]*?)```/g, (_, lang, code) => {
    codeBlocks.push(`<pre><code>${escapeHtml(code.trimEnd())}</code></pre>`);
    return `\x00CB${codeBlocks.length - 1}\x00`;
  });

  html = escapeHtml(html);

  html = html
    .replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_, alt, src) => {
      const safeSrc = isSafeUrl(src) ? src : ``;
      return safeSrc ? `<img src="${safeSrc}" alt="${alt}" loading="lazy"/>` : alt;
    });

  html = html
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, text, url) => {
      const safe = isSafeUrl(url) ? url : `#`;
      return `<a href="${safe}" target="_blank" rel="noopener noreferrer">${text}</a>`;
    });

  html = html
    .replace(/^######\s+(.+)$/gm, `<h6>$1</h6>`)
    .replace(/^#####\s+(.+)$/gm, `<h5>$1</h5>`)
    .replace(/^####\s+(.+)$/gm, `<h4>$1</h4>`)
    .replace(/^###\s+(.+)$/gm, `<h3>$1</h3>`)
    .replace(/^##\s+(.+)$/gm, `<h2>$1</h2>`)
    .replace(/^#\s+(.+)$/gm, `<h1>$1</h1>`);

  html = html
    .replace(/\*\*\*(.+?)\*\*\*/g, `<strong><em>$1</em></strong>`)
    .replace(/\*\*(.+?)\*\*/g, `<strong>$1</strong>`)
    .replace(/(?<!\w)\*(.+?)\*(?!\w)/g, `<em>$1</em>`);

  html = html
    .replace(/`([^`]+)`/g, `<code>$1</code>`);

  html = html
    .replace(/^&gt;\s+(.+)$/gm, `<blockquote><p>$1</p></blockquote>`);

  html = html
    .replace(/^---+$/gm, `<hr/>`);

  html = html
    .replace(/^[*-]\s+(.+)$/gm, `<li>$1</li>`)
    .replace(/((?:<li>[\s\S]*?<\/li>\s*)+)/g, `<ul>$1</ul>`);

  html = html
    .replace(/^\d+\.\s+(.+)$/gm, `<li>$1</li>`);

  const lines = html.split(`\n`);
  const result: Array<string> = [];
  let inBlock = false;
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) {
      if (inBlock) {
        result.push(``); inBlock = false;
      }
      continue;
    }
    if (trimmed.startsWith(`<`) || trimmed.startsWith(`\x00`)) {
      result.push(trimmed);
      inBlock = false;
    } else {
      result.push(`<p>${trimmed}</p>`);
      inBlock = true;
    }
  }
  html = result.join(`\n`);

  html = html.replace(/\x00CB(\d+)\x00/g, (_, idx) => codeBlocks[parseInt(idx)]);

  return html;
}

// ── File Tree Builder ──

export function buildFileTree(files: Array<FileEntry>, packageName: string): TreeNode {
  const root: TreeNode = {name: packageName, path: ``, children: []};

  for (const file of files) {
    const parts = file.name.split(`/`).filter(Boolean);
    let current = root;

    for (let i = 0; i < parts.length; i++) {
      const isFile = i === parts.length - 1;

      if (!current.children)
        current.children = [];

      let child = current.children.find(c => c.name === parts[i]);
      if (!child) {
        child = {
          name: parts[i],
          path: parts.slice(0, i + 1).join(`/`),
          ...(isFile ? {size: file.size} : {children: []}),
        };
        current.children.push(child);
      }

      current = child;
    }
  }

  sortTree(root);

  return root;
}

function sortTree(node: TreeNode): void {
  if (!node.children) return;
  node.children.sort((a, b) => {
    const aDir = !!a.children;
    const bDir = !!b.children;
    if (aDir !== bDir) return aDir ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  node.children.forEach(sortTree);
}

// ── Sparkline ──

export function sparklinePath(data: Array<number>, w: number, h: number): string {
  if (data.length < 2)
    return ``;

  const max = Math.max(...data);
  const min = Math.min(...data);
  const range = max - min || 1;
  const step = w / (data.length - 1);

  return data.map((v, i) => {
    const x = i * step;
    const y = h - ((v - min) / range) * (h * 0.85) - h * 0.05;
    return `${i === 0 ? `M` : `L`}${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(` `);
}

// ── Semver helpers ──

function parseVersion(v: string): {major: number, minor: number, patch: number, pre: string} {
  const match = v.match(/^(\d+)\.(\d+)\.(\d+)(.*)$/);
  if (!match) return {major: 0, minor: 0, patch: 0, pre: v};
  return {major: +match[1], minor: +match[2], patch: +match[3], pre: match[4]};
}

export function isNoisyPrerelease(v: string): boolean {
  const pre = v.replace(/^\d+\.\d+\.\d+[-.]?/, ``);
  if (!pre)
    return false;

  for (const seg of pre.split(/[.-]/)) {
    if (/[a-f0-9]{6,}/i.test(seg) && /[a-f]/i.test(seg) && /\d/.test(seg))
      return true;

    if (/^\d{8}$/.test(seg)) {
      return true;
    }
  }
  return false;
}

export function compareSemverDesc(a: string, b: string): number {
  const pa = parseVersion(a);
  const pb = parseVersion(b);
  if (pa.major !== pb.major) return pb.major - pa.major;
  if (pa.minor !== pb.minor) return pb.minor - pa.minor;
  if (pa.patch !== pb.patch) return pb.patch - pa.patch;
  if (!pa.pre && pb.pre) return -1;
  if (pa.pre && !pb.pre) return 1;
  return pa.pre < pb.pre ? 1 : pa.pre > pb.pre ? -1 : 0;
}

export function versionLabel(v: string, prev: string | null): string {
  if (!prev) return `initial`;
  const cur = parseVersion(v);
  const prv = parseVersion(prev);
  if (cur.major !== prv.major) return `major`;
  if (cur.minor !== prv.minor) return `minor`;
  return `patch`;
}

// ── PM install commands ──

export const PM_COMMANDS: Record<PmTab, {verb: string, rest: string}> = {
  yarn: {verb: `yarn`, rest: `add`},
  npm: {verb: `npm`, rest: `install`},
  pnpm: {verb: `pnpm`, rest: `add`},
  bun: {verb: `bun`, rest: `add`},
};

// ── Language detection ──

const EXT_LANG: Record<string, string> = {
  js: `javascript`, mjs: `javascript`, cjs: `javascript`,
  ts: `typescript`, mts: `typescript`, cts: `typescript`,
  jsx: `javascript`, tsx: `typescript`,
  json: `json`, json5: `json`,
  md: `markdown`, mdx: `markdown`,
  css: `css`, scss: `scss`, less: `less`,
  html: `html`, htm: `html`,
  yaml: `yaml`, yml: `yaml`,
  xml: `xml`, svg: `xml`,
  sh: `shell`, bash: `shell`, zsh: `shell`,
  py: `python`, rb: `ruby`, rs: `rust`, go: `go`,
  java: `java`, kt: `kotlin`, swift: `swift`,
  c: `c`, cpp: `cpp`, h: `c`, hpp: `cpp`,
  graphql: `graphql`, gql: `graphql`,
  sql: `sql`, toml: `ini`, ini: `ini`,
  txt: `plaintext`, log: `plaintext`,
};

export function langFromPath(filepath: string): string {
  const ext = filepath.split(`.`).pop()?.toLowerCase() ?? ``;
  return EXT_LANG[ext] ?? `plaintext`;
}

export function setupMonacoTheme(monaco: any) {
  monaco.editor.defineTheme(`pkg-dark`, {
    base: `vs-dark`,
    inherit: true,
    rules: [
      {token: `comment`, foreground: `6872a0`},
      {token: `keyword`, foreground: `c4a0f5`},
      {token: `string`, foreground: `a0dbb0`},
      {token: `number`, foreground: `d4c080`},
      {token: `type`, foreground: `90c8e8`},
      {token: `function`, foreground: `90c8e8`},
    ],
    colors: {
      'editor.background': `#0a0e28`,
      'editor.foreground': `#d6daf5`,
      'editor.lineHighlightBackground': `#ffffff08`,
      'editorLineNumber.foreground': `#6872a0`,
      'editorLineNumber.activeForeground': `#a8b0d4`,
      'editor.selectionBackground': `#ffffff18`,
      'editor.inactiveSelectionBackground': `#ffffff0d`,
      'editorIndentGuide.background': `#ffffff0a`,
      'editorIndentGuide.activeBackground': `#ffffff18`,
      'editorWidget.background': `#0a0e28`,
      'editorWidget.border': `#a8b0d424`,
      'scrollbarSlider.background': `#a8b0d428`,
      'scrollbarSlider.hoverBackground': `#a8b0d440`,
    },
  });
  monaco.editor.defineTheme(`pkg-light`, {
    base: `vs`,
    inherit: true,
    rules: [
      {token: `comment`, foreground: `7a84a8`},
      {token: `keyword`, foreground: `7030b0`},
      {token: `string`, foreground: `286840`},
      {token: `number`, foreground: `885510`},
      {token: `type`, foreground: `205878`},
      {token: `function`, foreground: `205878`},
    ],
    colors: {
      'editor.background': `#f2f5fc`,
      'editor.foreground': `#0c1030`,
      'editor.lineHighlightBackground': `#00000005`,
      'editorLineNumber.foreground': `#515a7a`,
      'editorLineNumber.activeForeground': `#252d50`,
      'editor.selectionBackground': `#0c103018`,
      'editor.inactiveSelectionBackground': `#0c10300d`,
      'editorIndentGuide.background': `#0c103010`,
      'editorIndentGuide.activeBackground': `#0c103020`,
      'editorWidget.background': `#f2f5fc`,
      'editorWidget.border': `#0c10301c`,
      'scrollbarSlider.background': `#0c103018`,
      'scrollbarSlider.hoverBackground': `#0c103028`,
    },
  });
}

// ── Prettier formatting ──

type PrettierCfg = {parser: string, load: () => Promise<Array<any>>};

const jsPlugins = () => Promise.all([import(`prettier/plugins/babel`), import(`prettier/plugins/estree`)]);
const tsPlugins = () => Promise.all([import(`prettier/plugins/typescript`), import(`prettier/plugins/estree`)]);
const cssPlugins = () => import(`prettier/plugins/postcss`).then(m => [m]);
const htmlPlugins = () => import(`prettier/plugins/html`).then(m => [m]);
const mdPlugins = () => import(`prettier/plugins/markdown`).then(m => [m]);
const yamlPlugins = () => import(`prettier/plugins/yaml`).then(m => [m]);

const PRETTIER: Record<string, PrettierCfg> = {
  js: {parser: `babel`, load: jsPlugins},
  jsx: {parser: `babel`, load: jsPlugins},
  mjs: {parser: `babel`, load: jsPlugins},
  cjs: {parser: `babel`, load: jsPlugins},
  ts: {parser: `typescript`, load: tsPlugins},
  tsx: {parser: `typescript`, load: tsPlugins},
  mts: {parser: `typescript`, load: tsPlugins},
  cts: {parser: `typescript`, load: tsPlugins},
  json: {parser: `json`, load: jsPlugins},
  css: {parser: `css`, load: cssPlugins},
  scss: {parser: `scss`, load: cssPlugins},
  less: {parser: `less`, load: cssPlugins},
  html: {parser: `html`, load: htmlPlugins},
  htm: {parser: `html`, load: htmlPlugins},
  md: {parser: `markdown`, load: mdPlugins},
  markdown: {parser: `markdown`, load: mdPlugins},
  yaml: {parser: `yaml`, load: yamlPlugins},
  yml: {parser: `yaml`, load: yamlPlugins},
};

export async function formatWithPrettier(code: string, filepath: string): Promise<string> {
  const ext = filepath.split(`.`).pop()?.toLowerCase() ?? ``;

  const cfg = PRETTIER[ext];
  if (!cfg)
    return code;

  try {
    const [prettier, plugins] = await Promise.all([
      import(`prettier/standalone`),
      cfg.load(),
    ]);
    return await prettier.format(code, {parser: cfg.parser, plugins});
  } catch (err) {
    console.error(`[prettier] failed to format ${filepath}:`, err);
    return code;
  }
}

export function canPrettify(filepath: string): boolean {
  const ext = filepath.split(`.`).pop()?.toLowerCase() ?? ``;
  return ext in PRETTIER;
}
