import {Cli}           from 'clipanion';
import {glob}          from 'glob';
import {createRequire} from 'module';
import path            from 'path';

interface BinaryConfig {
  name: string;
  getCli: () => Promise<Cli<any>>;
}

const BINARIES_CONFIG: Array<BinaryConfig> = [
  {
    name: `@yarnpkg/cli`,
    getCli: async () => {
      const pkg = await import(`@yarnpkg/cli`);

      if (pkg?.getCli)
        return pkg.getCli();


      throw new Error(`@yarnpkg/cli doesn't export a getCli function`);
    },
  },
  {
    name: `@yarnpkg/builder`,
    getCli: async () => {
      const pkg = await import(`@yarnpkg/builder`);

      const commands = Object.values(pkg).filter(
        command => typeof command === `function`,
      );

      return Cli.from(commands, {binaryName: `yarn builder`});
    },
  },
  {
    name: `@yarnpkg/pnpify`,
    getCli: async () => {
      const require = createRequire(import.meta.url);
      const packageJsonPath = require.resolve(`@yarnpkg/pnpify/package.json`);
      const packageDir = path.dirname(packageJsonPath);

      const patterns = [`${packageDir}/**/commands/**/*.js`];

      for (const pattern of patterns) {
        const commandPaths = await glob(pattern, {nodir: true});

        if (commandPaths.length > 0) {
          const commands = commandPaths.map(p => {
            console.log(`Loading:`, p);
            return require(p).default;
          });
          return Cli.from(commands, {binaryName: `yarn pnpify`});
        }
      }

      throw new Error(`No commands found in @yarnpkg/pnpify`);
    },
  },
  {
    name: `@yarnpkg/sdks`,
    getCli: async () => {
      const {SdkCommand} = await import(`@yarnpkg/sdks`);
      return Cli.from(SdkCommand, {binaryName: `yarn sdks`});
    },
  },
];

export const cliReferencePlugin = async function () {
  return {
    name: `CLI Reference`,
    async loadContent() {
      const results = await Promise.all(
        BINARIES_CONFIG.map(async ({name, getCli}) => {
          try {
            const cli = await getCli();
            return {name, definitions: cli.definitions()};
          } catch (error) {
            console.error(`Failed to load CLI definitions for ${name}:`, error);
            return {name, definitions: null};
          }
        }),
      );

      return Object.fromEntries(
        results.map(({name, definitions}) => [name, definitions]),
      );
    },
  };
};
