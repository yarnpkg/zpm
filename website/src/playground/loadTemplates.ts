import {readdir, readFile, stat} from 'node:fs/promises';
import path                    from 'node:path';
import {fileURLToPath}         from 'node:url';

import type {PlaygroundEntry, PlaygroundTemplate, PlaygroundTemplateManifest} from './types';

const templateManifestName = `_template.json`;

const languageByExtension = new Map([
  [`.css`, `css`],
  [`.cjs`, `javascript`],
  [`.html`, `html`],
  [`.js`, `javascript`],
  [`.json`, `json`],
  [`.jsx`, `javascript`],
  [`.md`, `markdown`],
  [`.mjs`, `javascript`],
  [`.mts`, `typescript`],
  [`.ts`, `typescript`],
  [`.tsx`, `typescript`],
  [`.yaml`, `yaml`],
  [`.yml`, `yaml`],
]);

type LoadPlaygroundTemplatesOptions = {
  yarnVersion: string;
};

type TemplateCandidate = {
  order: number;
  template: PlaygroundTemplate;
};

async function resolveTemplatesPath() {
  const candidates = [
    path.join(process.cwd(), `src/playground/templates`),
    path.join(process.cwd(), `website/src/playground/templates`),
    fileURLToPath(new URL(`./templates`, import.meta.url)),
  ];

  for (const candidate of candidates) {
    try {
      if ((await stat(candidate)).isDirectory())
        return candidate;
    } catch {
      // Keep trying the other known source locations.
    }
  }

  throw new Error(`Couldn't find playground templates directory`);
}

function assertTemplateManifest(folder: string, value: unknown): asserts value is PlaygroundTemplateManifest {
  if (typeof value !== `object` || value === null)
    throw new Error(`${folder}/${templateManifestName} must contain a JSON object`);

  const manifest = value as Record<string, unknown>;

  if (typeof manifest.label !== `string` || manifest.label.length === 0)
    throw new Error(`${folder}/${templateManifestName} must define a non-empty "label"`);

  if (manifest.description !== undefined && typeof manifest.description !== `string`)
    throw new Error(`${folder}/${templateManifestName} "description" must be a string when set`);

  if (manifest.order !== undefined && typeof manifest.order !== `number`)
    throw new Error(`${folder}/${templateManifestName} "order" must be a number when set`);
}

function compareNames(a: string, b: string) {
  return a.localeCompare(b, `en`, {numeric: true});
}

function compareEntries(a: {isDirectory(): boolean, name: string}, b: {isDirectory(): boolean, name: string}) {
  if (a.isDirectory() !== b.isDirectory())
    return a.isDirectory() ? 1 : -1;

  return compareNames(a.name, b.name);
}

function getLanguage(filePath: string) {
  const basename = path.basename(filePath);

  if (basename === `.yarnrc.yml`)
    return `yaml`;

  return languageByExtension.get(path.extname(basename));
}

function normalizeTemplateContent(content: string, {yarnVersion}: LoadPlaygroundTemplatesOptions) {
  return content.replaceAll(`{{YARN_VERSION}}`, yarnVersion);
}

async function loadTemplateEntries(
  absoluteFolder: string,
  relativeFolder: string,
  depth: number,
  options: LoadPlaygroundTemplatesOptions,
): Promise<Array<PlaygroundEntry>> {
  const entries: Array<PlaygroundEntry> = [];
  const dirents = (await readdir(absoluteFolder, {withFileTypes: true}))
    .filter(dirent => dirent.name !== templateManifestName)
    .sort(compareEntries);

  for (const dirent of dirents) {
    const entryPath = relativeFolder ? `${relativeFolder}/${dirent.name}` : dirent.name;
    const absolutePath = path.join(absoluteFolder, dirent.name);

    if (dirent.isDirectory()) {
      const children = await loadTemplateEntries(absolutePath, entryPath, depth + 1, options);

      if (children.length === 0)
        continue;

      entries.push({
        depth,
        kind: `folder`,
        name: dirent.name,
        path: entryPath,
      });

      entries.push(...children);
    } else if (dirent.isFile()) {
      entries.push({
        content: normalizeTemplateContent(await readFile(absolutePath, `utf8`), options),
        depth,
        kind: `file`,
        language: getLanguage(entryPath),
        name: dirent.name,
        path: entryPath,
      });
    }
  }

  return entries;
}

async function loadTemplate(templatesPath: string, folder: string, options: LoadPlaygroundTemplatesOptions): Promise<TemplateCandidate> {
  const absoluteFolder = path.join(templatesPath, folder);
  const manifestPath = path.join(absoluteFolder, templateManifestName);
  const manifest = JSON.parse(await readFile(manifestPath, `utf8`)) as unknown;

  assertTemplateManifest(folder, manifest);

  return {
    order: manifest.order ?? Number.MAX_SAFE_INTEGER,
    template: {
      description: manifest.description ?? ``,
      entries: await loadTemplateEntries(absoluteFolder, ``, 0, options),
      id: folder,
      label: manifest.label,
    },
  };
}

export async function loadPlaygroundTemplates(options: LoadPlaygroundTemplatesOptions): Promise<Array<PlaygroundTemplate>> {
  const templatesPath = await resolveTemplatesPath();
  const templateFolders = (await readdir(templatesPath, {withFileTypes: true}))
    .filter(dirent => dirent.isDirectory())
    .map(dirent => dirent.name)
    .sort(compareNames);

  const templates = await Promise.all(templateFolders.map(folder => loadTemplate(templatesPath, folder, options)));

  return templates
    .sort((a, b) => a.order - b.order || compareNames(a.template.label, b.template.label))
    .map(({template}) => template);
}
