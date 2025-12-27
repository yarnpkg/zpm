import {Yarn}                        from '@yarnpkg/types';
import {readFileSync, writeFileSync} from 'fs';
import get                           from 'lodash/get';
import {createRequire}               from 'module';
import {join}                        from 'path';

import * as constraintsUtils         from './constraintsUtils';
import * as miscUtils                from './miscUtils';
import * as nodeUtils                from './nodeUtils';

export type AnnotatedError =
  | {type: `missingField`, fieldPath: Array<string>, expected: any}
  | {type: `extraneousField`, fieldPath: Array<string>, currentValue: any}
  | {type: `invalidField`, fieldPath: Array<string>, expected: any, currentValue: any}
  | {type: `conflictingValues`, fieldPath: Array<string>, setValues: Array<[any, PerValueInfo]>, unsetValues: PerValueInfo | null}
  | {type: `userError`, message: string};

export type Operation =
  | {type: `set`, path: Array<string>, value: any}
  | {type: `unset`, path: Array<string>};

type PerValueInfo = {
  callers: Array<nodeUtils.Caller>;
};

type PerPathInfo = {
  fieldPath: Array<string>;
  values: Map<any, PerValueInfo>;
};

const allWorkspaceActions = new Map<string, {
  updates: Map<string, PerPathInfo>;
  errors: Array<AnnotatedError>;
}>();

const workspaceIndex = new constraintsUtils.Index<Yarn.Constraints.Workspace>([`cwd`, `ident`]);
const dependencyIndex = new constraintsUtils.Index<Yarn.Constraints.Dependency>([`workspace`, `type`, `ident`]);
const packageIndex = new constraintsUtils.Index<Yarn.Constraints.Package>([`ident`]);

const createSetFn = (workspaceCwd: string) => (path: Array<string> | string, value: any, {caller = nodeUtils.getCaller()}: {caller?: nodeUtils.Caller | null} = {}) => {
  const pathfieldPath = constraintsUtils.normalizePath(path);
  const key = pathfieldPath.join(`.`);

  const workspaceActions = miscUtils.getFactoryWithDefault(allWorkspaceActions, workspaceCwd, () => ({
    updates: new Map(),
    errors: [],
  }));

  const pathUpdates = miscUtils.getFactoryWithDefault(workspaceActions.updates, key, () => ({
    fieldPath: pathfieldPath,
    values: new Map(),
  }));

  const constraints = miscUtils.getFactoryWithDefault(pathUpdates.values, value, () => ({
    callers: [],
  }));

  if (caller !== null) {
    constraints.callers.push(caller);
  }
};

const createErrorFn = (workspaceCwd: string) => (message: string) => {
  const workspaceActions = miscUtils.getFactoryWithDefault(allWorkspaceActions, workspaceCwd, () => ({
    updates: new Map(),
    errors: [],
  }));

  workspaceActions.errors.push({
    type: `userError`,
    message,
  });
};

declare const SERIALIZED_CONTEXT: string;
declare const CONFIG_PATH: string;
declare const FIX: boolean;

const RESULT_PATH = process.argv[2]!;

type InputDependency = {
  ident: string;
  range: string;
  dependencyType: Yarn.Constraints.DependencyType;
  resolution: string | null;
};

const input: {
  workspaces: Array<{
    cwd: string;
    ident: string;
    dependencies: Array<InputDependency>;
    peerDependencies: Array<InputDependency>;
    devDependencies: Array<InputDependency>;
  }>;
  packages: Array<{
    locator: string;
    workspace: string | null;
    ident: string;
    version: string;
    dependencies: Array<[string, string]>;
    peerDependencies: Array<[string, string]>;
    optionalPeerDependencies: Array<[string, string]>;
  }>;
} = JSON.parse(SERIALIZED_CONTEXT);

const packageByLocator = new Map<string, Yarn.Constraints.Package>();
const workspaceByCwd = new Map<string, Yarn.Constraints.Workspace>();

for (const workspace of input.workspaces) {
  const setFn = createSetFn(workspace.cwd);
  const errorFn = createErrorFn(workspace.cwd);

  const unsetFn = (path: Array<string> | string) => {
    return setFn(path, undefined, {caller: nodeUtils.getCaller()});
  };

  const manifestPath = join(workspace.cwd, `package.json`);
  const manifestContent = readFileSync(manifestPath, `utf8`);
  const manifest = JSON.parse(manifestContent);

  const hydratedWorkspace: Yarn.Constraints.Workspace = {
    cwd: workspace.cwd,
    ident: workspace.ident,
    manifest,
    pkg: null as any,
    set: setFn,
    unset: unsetFn,
    error: errorFn,
  };

  workspaceByCwd.set(workspace.cwd, hydratedWorkspace);
  workspaceIndex.insert(hydratedWorkspace);
}

for (const pkg of input.packages) {
  const workspace = pkg.workspace !== null
    ? workspaceByCwd.get(pkg.workspace)!
    : null;

  if (typeof workspace === `undefined`)
    throw new Error(`Workspace ${pkg.workspace} not found`);

  const hydratedPackage: Yarn.Constraints.Package = {
    ident: pkg.ident,
    workspace,
    version: pkg.version,
    dependencies: new Map(),
    peerDependencies: new Map(pkg.peerDependencies),
    optionalPeerDependencies: new Map(pkg.optionalPeerDependencies),
  };

  if (workspace !== null)
    workspace.pkg = hydratedPackage;

  packageByLocator.set(pkg.locator, hydratedPackage);
  packageIndex.insert(hydratedPackage);
}

for (const workspace of input.workspaces) {
  const setFn = createSetFn(workspace.cwd);
  const errorFn = createErrorFn(workspace.cwd);

  const hydratedWorkspace = workspaceByCwd.get(workspace.cwd);
  if (typeof hydratedWorkspace === `undefined`)
    throw new Error(`Workspace ${workspace.cwd} not found`);

  for (const dependency of workspace.dependencies) {
    const resolution = dependency.resolution !== null
      ? packageByLocator.get(dependency.resolution)!
      : null;

    if (typeof resolution === `undefined`)
      throw new Error(`Dependency ${dependency.ident}@${dependency.range} (resolution: ${dependency.resolution}) not found`);

    const hydratedDependency: Yarn.Constraints.Dependency = {
      workspace: hydratedWorkspace,
      ident: dependency.ident,
      range: dependency.range,
      type: dependency.dependencyType,
      resolution,
      update: range => {
        setFn([dependency.dependencyType, dependency.ident], range, {caller: nodeUtils.getCaller()});
      },
      delete: () => {
        setFn([dependency.dependencyType, dependency.ident], undefined, {caller: nodeUtils.getCaller()});
      },
      error: errorFn,
    };

    dependencyIndex.insert(hydratedDependency);
  }

  for (const peerDependency of workspace.peerDependencies) {
    const hydratedPeerDependency: Yarn.Constraints.Dependency = {
      workspace: hydratedWorkspace,
      ident: peerDependency.ident,
      range: peerDependency.range,
      type: `peerDependencies`,
      resolution: null,
      update: () => {
        setFn([`peerDependencies`, peerDependency.ident], peerDependency.range, {caller: nodeUtils.getCaller()});
      },
      delete: () => {
        setFn([`peerDependencies`, peerDependency.ident], undefined, {caller: nodeUtils.getCaller()});
      },
      error: errorFn,
    };

    dependencyIndex.insert(hydratedPeerDependency);
  }

  for (const devDependency of workspace.devDependencies) {
    const resolution = devDependency.resolution !== null
      ? packageByLocator.get(devDependency.resolution)!
      : null;

    if (typeof resolution === `undefined`)
      throw new Error(`Dependency ${devDependency.ident} not found`);

    const hydratedDevDependency: Yarn.Constraints.Dependency = {
      workspace: hydratedWorkspace,
      ident: devDependency.ident,
      range: devDependency.range,
      type: `devDependencies`,
      resolution,
      update: () => {
        setFn([`devDependencies`, devDependency.ident], devDependency.range, {caller: nodeUtils.getCaller()});
      },
      delete: () => {
        setFn([`devDependencies`, devDependency.ident], undefined, {caller: nodeUtils.getCaller()});
      },
      error: errorFn,
    };

    dependencyIndex.insert(hydratedDevDependency);
  }
}

for (const pkg of input.packages) {
  const hydratedPackage = packageByLocator.get(pkg.locator)!;

  for (const [dependency, locator] of pkg.dependencies) {
    hydratedPackage.dependencies.set(dependency, packageByLocator.get(locator)!);
  }
}

const context: Yarn.Constraints.Context = {
  Yarn: {
    workspace: ((filter?: Yarn.Constraints.WorkspaceFilter) => {
      return workspaceIndex.find(filter)[0] ?? null;
    }) as any,
    workspaces: filter => {
      return workspaceIndex.find(filter);
    },

    dependency: ((filter?: Yarn.Constraints.DependencyFilter) => {
      return dependencyIndex.find(filter)[0] ?? null;
    }) as any,
    dependencies: filter => {
      return dependencyIndex.find(filter);
    },

    package: ((filter?: Yarn.Constraints.PackageFilter) => {
      return packageIndex.find(filter)[0] ?? null;
    }) as any,
    packages: filter => {
      return packageIndex.find(filter);
    },
  },
};

function applyEngineReport(fix: boolean) {
  const allWorkspaceOperations = new Map<string, Array<Operation>>();
  const allWorkspaceErrors = new Map<string, Array<AnnotatedError>>();

  for (const [workspaceCwd, workspaceActions] of allWorkspaceActions) {
    const manifest = workspaceByCwd.get(workspaceCwd)!.manifest;

    const workspaceErrors = workspaceActions.errors.slice();
    const workspaceOperations: Array<Operation> = [];

    for (const {fieldPath, values} of workspaceActions.updates.values()) {
      const valuesArray = [...values];
      if (valuesArray.length === 0)
        continue;

      if (valuesArray.length > 1) {
        const unsetValues = valuesArray
          .filter(([value]) => typeof value === `undefined`)
          ?.[0]?.[1] ?? null;

        const setValues = valuesArray
          .filter(([value]) => typeof value !== `undefined`);

        workspaceErrors.push({
          type: `conflictingValues`,
          fieldPath,
          setValues,
          unsetValues,
        });
      } else {
        const newValue = valuesArray[0]![0];

        const currentValue = get(manifest, fieldPath);
        if (JSON.stringify(currentValue) === JSON.stringify(newValue))
          continue;

        if (!fix) {
          const error: AnnotatedError = typeof currentValue === `undefined`
            ? {type: `missingField`, fieldPath, expected: newValue}
            : typeof newValue === `undefined`
              ? {type: `extraneousField`, fieldPath, currentValue}
              : {type: `invalidField`, fieldPath, expected: newValue, currentValue};

          workspaceErrors.push(error);
          continue;
        }

        if (typeof newValue === `undefined`) {
          workspaceOperations.push({type: `unset`, path: fieldPath});
        } else {
          workspaceOperations.push({type: `set`, path: fieldPath, value: newValue});
        }
      }
    }

    if (workspaceOperations.length > 0)
      allWorkspaceOperations.set(workspaceCwd, workspaceOperations);


    if (workspaceErrors.length > 0) {
      allWorkspaceErrors.set(workspaceCwd, workspaceErrors);
    }
  }

  return {
    allWorkspaceOperations: [...allWorkspaceOperations],
    allWorkspaceErrors: [...allWorkspaceErrors],
  };
}

async function main() {
  const require = createRequire(CONFIG_PATH);

  const config = require(CONFIG_PATH) as Yarn.Config & {default?: Yarn.Config};

  const defaultConfig = config?.default ?? config;
  await defaultConfig.constraints?.(context);

  const output = applyEngineReport(FIX);

  writeFileSync(RESULT_PATH, JSON.stringify(output, null, 2));
}

main();
