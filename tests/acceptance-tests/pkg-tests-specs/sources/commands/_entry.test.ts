import {PortablePath, npath, xfs} from '@yarnpkg/fslib';

type Documentation = {description: string, details: string | null};
type CommandSpec = {primaryPath: Array<string>, documentation: Documentation | null, examples: Array<{command: string, description: string}>, components: Array<{type: string, primaryName?: string, documentation?: Documentation | null}>};

// eslint-disable-next-line no-control-regex
const ANSI_REGEXP = /\x1b\[[0-9;]*m/g;

function normalizeOutput(output: string) {
  return output.replace(ANSI_REGEXP, ``).replace(/\r\n/g, `\n`);
}

// The help wraps its text: we compare it to the documentation once their
// whitespace is collapsed
function collapseWhitespace(text: string) {
  return text.replace(/\s+/g, ` `).trim();
}

function getSection(help: string, title: string) {
  const match = help.match(new RegExp(`^━━━ ${title} ━+\\n([^]*?)(?=^━━━ |(?![^]))`, `m`));
  if (!match)
    throw new Error(`Section ${title} not found in:\n${help}`);

  return match[1] ?? ``;
}

async function getCommandSpec(run: (...args: Array<string>) => Promise<{stdout: string}>, path: Array<string>) {
  const {stdout} = await run(`--clipanion-commands`);
  const commands: Array<CommandSpec> = JSON.parse(stdout);

  const command = commands.find(command => command.primaryPath.join(` `) === path.join(` `));
  if (!command)
    throw new Error(`Command ${path.join(` `)} not found`);

  return command;
}

describe(`Entry`, () => {
  describe(`--help`, () => {
    test(
      `it should print the description, usage, options and details of a command`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const spec = await getCommandSpec(run, [`add`]);
        const help = normalizeOutput((await run(`add`, `--help`)).stdout);

        expect(help.startsWith(`${spec.documentation!.description}\n`)).toBe(true);

        const sections = [...help.matchAll(/^━━━ (\w+) ━+$/gm)].map(match => match[1]);
        expect(sections.slice(0, 3)).toEqual([`Usage`, `Options`, `Details`]);

        expect(getSection(help, `Usage`)).toMatch(/^ *› yarn add .*--dev,-D/m);

        const options = getSection(help, `Options`);
        expect(options).toMatch(/^ {2}--dev,-D +\S/m);
        expect(options).toMatch(/^ {2}--mode <arg> +\S/m);

        // The examples are documented at the end of the details, under an
        // `## Examples` heading
        const [details = ``, examples = ``] = spec.documentation!.details!.split(/\n\n\s*## Examples\s*\n\n/);
        expect(collapseWhitespace(getSection(help, `Details`))).toContain(collapseWhitespace(details));

        const exampleCommands = [
          ...spec.examples.map(example => example.command),
          ...[...examples.matchAll(/^\s*`(.+)`\s*$/gm)].map(match => match[1]),
        ];

        if (exampleCommands.length > 0) {
          const examplesSection = getSection(help, `Examples`);
          for (const command of exampleCommands) {
            expect(examplesSection).toContain(`› ${command}\n`);
          }
        }
      }),
    );

    test(
      `it should print the whole description of an option documented over several lines`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const spec = await getCommandSpec(run, [`import`, `uv`]);
        const help = normalizeOutput((await run(`import`, `uv`, `--help`)).stdout);

        const option = spec.components.find(component => component.type === `option` && component.primaryName === `--extra`)!;
        const description = [option.documentation!.description, option.documentation!.details].filter(Boolean).join(` `);

        const entry = getSection(help, `Options`).match(/^ {2}--extra .*\n(?: {3,}\S.*\n)*/m)?.[0];
        expect(entry).toBeDefined();

        expect(collapseWhitespace(entry!)).toContain(collapseWhitespace(description));
        expect(collapseWhitespace(entry!)).toContain(`as \`uv sync --extra\` does`);
      }),
    );

    test(
      `it should print the help of a command when given -h`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const longHelp = normalizeOutput((await run(`remove`, `--help`)).stdout);
        const shortHelp = normalizeOutput((await run(`remove`, `-h`)).stdout);

        expect(shortHelp).toEqual(longHelp);
        expect(longHelp).toMatch(/^━━━ Details ━+$/m);
      }),
    );

    test(
      `it should still list the commands when given --help alone`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const help = normalizeOutput((await run(`--help`)).stdout);

        expect(help).toMatch(/^━━━ Dependency management ━+$/m);
        expect(help).not.toMatch(/^━━━ Details ━+$/m);
      }),
    );
  });

  describe(`--version`, () => {
    const versionPattern = /^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/;

    test(
      `it should print the binary version when given --version`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const {stdout} = await run(`--version`);
        expect(stdout.trim()).toMatch(versionPattern);
      }),
    );

    test(
      `it should print the binary version when given -v`,
      makeTemporaryEnv({}, async ({path, run, source}) => {
        const {stdout} = await run(`-v`);
        expect(stdout.trim()).toMatch(versionPattern);
      }),
    );
  });

  describe(`--cwd`, () => {
    test(`should use the specified --cwd (relative path)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`--cwd`, `packages`, `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));

    test(`should use the specified --cwd (absolute path)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`--cwd`, npath.fromPortablePath(`${path}/packages`), `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));

    test(`should use the specified --cwd (relative symlink)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);
      await xfs.symlinkPromise(`${path}/packages` as PortablePath, `${path}/modules` as PortablePath);

      await expect(run(`--cwd`, `modules`, `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/modules`)}\n`,
      });
    }));

    test(`should use the specified --cwd (absolute symlink)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);
      await xfs.symlinkPromise(`${path}/packages` as PortablePath, `${path}/modules` as PortablePath);

      await expect(run(`--cwd`, npath.fromPortablePath(`${path}/modules`), `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/modules`)}\n`,
      });
    }));

    test(`should use the specified --cwd (bound relative path)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`--cwd=packages`, `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));

    test(`should use the specified --cwd (bound absolute path)`, makeTemporaryEnv({}, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`--cwd=${npath.fromPortablePath(`${path}/packages`)}`, `exec`, `pwd`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));

    test(`should use the specified --cwd (inside script)`, makeTemporaryEnv({
      scripts: {
        foo: `yarn --cwd=packages exec pwd`,
      },
    }, async ({path, run, source}) => {
      await run(`install`);

      await xfs.mkdirPromise(`${path}/packages` as PortablePath);

      await expect(run(`run`, `foo`)).resolves.toMatchObject({
        stdout: `${npath.fromPortablePath(`${path}/packages`)}\n`,
      });
    }));
  });
});
