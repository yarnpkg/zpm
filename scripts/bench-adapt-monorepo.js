// Usage: node bench-adapt-monorepo.js <pnpm|npm|classic> [pnpm-major]
//
// Rewrites the monorepo fixture (a Yarn project) found in the current
// directory so that another package manager can install it. Each package
// manager gets the smallest set of changes that makes it install the same
// workspaces and the same registry dependencies; the differences that remain
// are listed next to each adapter below.

const fs = require(`fs`);
const path = require(`path`);

const packageManager = process.argv[2];
const pnpmMajor = Number(process.argv[3]);

const DEPENDENCY_FIELDS = [`dependencies`, `devDependencies`, `peerDependencies`, `optionalDependencies`];

const readManifest = manifestPath => {
  return JSON.parse(fs.readFileSync(manifestPath, `utf8`));
};

const writeManifest = (manifestPath, manifest) => {
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
};

const findManifests = dir => {
  const manifests = [];

  for (const entry of fs.readdirSync(dir, {withFileTypes: true})) {
    if (entry.name === `node_modules` || entry.name.startsWith(`.`))
      continue;

    const entryPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      manifests.push(...findManifests(entryPath));
    } else if (entry.name === `package.json`) {
      manifests.push(entryPath);
    }
  }

  return manifests;
};

// Calls `fn(range, name, manifestPath)` for every dependency of every
// manifest of the project, and replaces the range by the returned value
// (or removes the dependency if `null`)
const mapRanges = fn => {
  for (const manifestPath of findManifests(`.`)) {
    const manifest = readManifest(manifestPath);
    let changed = false;

    for (const field of DEPENDENCY_FIELDS) {
      for (const [name, range] of Object.entries(manifest[field] ?? {})) {
        const mapped = fn(range, name, manifestPath);
        if (mapped === range)
          continue;

        if (mapped === null)
          delete manifest[field][name];
        else
          manifest[field][name] = mapped;


        changed = true;
      }
    }

    if (changed) {
      writeManifest(manifestPath, manifest);
    }
  }
};

const splitResolution = key => {
  const index = key.lastIndexOf(`@`);

  return index > 0
    ? {name: key.slice(0, index), range: key.slice(index + 1)}
    : {name: key, range: null};
};

// pnpm refuses to install a project whose `packageManager` field names
// another package manager, doesn't read the `workspaces` field, doesn't
// support path-based `workspace:` ranges, and reads overrides from
// pnpm-workspace.yaml rather than from `resolutions`.
const adaptForPnpm = () => {
  const manifest = readManifest(`package.json`);
  delete manifest.packageManager;

  const workspaceYaml = [
    `packages:`,
    ...manifest.workspaces.map(pattern => `  - ${JSON.stringify(pattern)}`),
  ];

  if (manifest.resolutions) {
    if (pnpmMajor >= 11) {
      workspaceYaml.push(`overrides: ${JSON.stringify(manifest.resolutions)}`);
    } else {
      manifest.pnpm = {...manifest.pnpm, overrides: manifest.resolutions};
    }
  }

  writeManifest(`package.json`, manifest);
  fs.appendFileSync(`pnpm-workspace.yaml`, `${workspaceYaml.join(`\n`)}\n`);

  mapRanges(range => {
    return range.startsWith(`workspace:`) && range.includes(`/`) ? `workspace:*` : range;
  });
};

// npm and Yarn Classic don't support the `workspace:` protocol: they link a
// workspace whenever its version satisfies the requested range. None of the
// fixture's workspaces have a version, so they all get one and are depended
// upon through `*`.
const versionWorkspaces = () => {
  for (const manifestPath of findManifests(`.`)) {
    const manifest = readManifest(manifestPath);
    if (typeof manifest.version === `undefined`) {
      writeManifest(manifestPath, {...manifest, version: `0.0.0`});
    }
  }
};

// A few workspaces depend on the root workspace, which neither npm nor Yarn
// Classic can link (they'd look for it on the registry); those dependencies
// are removed.
const mapWorkspaceRange = (range, name, rootName) => {
  if (!range.startsWith(`workspace:`))
    return range;

  return name === rootName ? null : `*`;
};

// npm doesn't support `link:` either. It symlinks `file:` folders, but
// requires them to exist and to contain a manifest (Yarn and pnpm are fine
// with the fixture's dangling links). `resolutions` become `overrides`, which
// accept the same `name@range` keys; npm rejects overrides that disagree with
// a dependency of the root workspace, so those are applied to the root
// dependencies themselves (which is what Yarn's resolutions do).
const adaptForNpm = () => {
  const rootName = readManifest(`package.json`).name;

  versionWorkspaces();

  mapRanges((range, name, manifestPath) => {
    if (!range.startsWith(`link:`))
      return mapWorkspaceRange(range, name, rootName);

    const linkPath = range.slice(`link:`.length);
    const targetPath = path.join(path.dirname(manifestPath), linkPath);

    fs.mkdirSync(targetPath, {recursive: true});
    if (!fs.existsSync(path.join(targetPath, `package.json`)))
      writeManifest(path.join(targetPath, `package.json`), {name, version: `0.0.0`});

    return `file:${linkPath}`;
  });

  const manifest = readManifest(`package.json`);
  delete manifest.packageManager;

  const {resolutions = {}} = manifest;
  delete manifest.resolutions;

  manifest.overrides = {};

  for (const [key, value] of Object.entries(resolutions)) {
    const {name, range} = splitResolution(key);

    const field = [`dependencies`, `devDependencies`].find(field => {
      return typeof manifest[field]?.[name] !== `undefined`;
    });

    if (typeof field === `undefined`) {
      manifest.overrides[key] = value;
    } else if (range === null || range === manifest[field][name]) {
      manifest[field][name] = value;
      manifest.overrides[name] = `$${name}`;
    }
  }

  writeManifest(`package.json`, manifest);
};

// Yarn Classic only enables workspaces in private projects, and its
// `resolutions` only match packages by name (or by path): the entries
// targeting a specific range of a package can't be expressed and are dropped,
// leaving the packages they cover on the version their dependents ask for.
const adaptForClassic = () => {
  const rootName = readManifest(`package.json`).name;

  versionWorkspaces();

  mapRanges((range, name) => {
    return mapWorkspaceRange(range, name, rootName);
  });

  const manifest = readManifest(`package.json`);
  delete manifest.packageManager;

  manifest.private = true;

  manifest.resolutions = Object.fromEntries(Object.entries(manifest.resolutions ?? {}).filter(([key]) => {
    return splitResolution(key).range === null;
  }));

  writeManifest(`package.json`, manifest);
};

const adapters = {
  pnpm: adaptForPnpm,
  npm: adaptForNpm,
  classic: adaptForClassic,
};

if (!Object.hasOwn(adapters, packageManager)) {
  console.error(`No monorepo adapter for ${packageManager}`);
  process.exit(1);
}

adapters[packageManager]();
