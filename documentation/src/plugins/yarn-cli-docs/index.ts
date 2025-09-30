import path                                       from 'node:path';

import {cliReferencePlugin}                       from './binaries';
import {createCommandContent, createIndexContent} from './content-builder';

import {
  createFileIfNotExists,
  ensureDirectoryExists,
} from './helpers';

async function createYarnCliDocs(): Promise<void> {
  try {
    const cliPlugin = await cliReferencePlugin();
    const cliContent = await cliPlugin.loadContent();
    const cliBaseDir = path.join(`src/content/docs`, `cli`);

    // Ensure the cli directory exists first
    await ensureDirectoryExists(cliBaseDir);

    // Create _meta.yml for the main cli directory
    const mainMetaContent = `label: CLI Commands`;
    await createFileIfNotExists(
      path.join(cliBaseDir, `_meta.yml`),
      mainMetaContent,
    );

    await Promise.all(
      Object.entries(cliContent).map(async ([commandName, commandData]) => {
        if (!commandData) return;

        const shortName = commandName.split(`/`)[1];

        // Create subdirectory for each package
        const commandFolder = path.join(cliBaseDir, shortName);
        await ensureDirectoryExists(commandFolder);

        // Create _meta.yml for each subdirectory with order
        const metaLabels = {
          cli: `@yarnpkg/cli`,
          builder: `@yarnpkg/builder`,
          pnpify: `@yarnpkg/pnpify`,
          sdks: `@yarnpkg/sdks`,
        };

        // Get sorted keys to determine order
        const sortedKeys = Object.keys(metaLabels).sort();
        const order = sortedKeys.indexOf(shortName) + 1;

        const metaContent = `label: "${
          metaLabels[shortName as keyof typeof metaLabels] || shortName
        }"
order: ${order}`;
        await createFileIfNotExists(
          path.join(commandFolder, `_meta.yml`),
          metaContent,
        );

        const indexFilePath = path.join(commandFolder, `index.mdx`);
        const commands = Object.values(commandData).map(cmd => ({
          path: cmd.path,
          description: cmd.description || `No description available`,
        }));

        const indexContent = createIndexContent(
          commandName,
          shortName,
          commands,
        );

        await createFileIfNotExists(indexFilePath, indexContent);

        // Generate commands with proper slugs and order
        await Promise.all(
          Object.values(commandData).map(async (commandInfo, index) => {
            const fileName =
              commandInfo.path.split(` `).slice(1).join(`-`) ??
              commandInfo.path;

            const filePath = path.join(commandFolder, `${fileName}.mdx`);
            const fileContent = createCommandContent(
              commandName,
              shortName,
              commandInfo,
            );
            await createFileIfNotExists(filePath, fileContent);
          }),
        );
      }),
    );

    console.log(`CLI documentation generation completed`);
  } catch (error) {
    console.error(`Failed to generate CLI documentation:`, error);
    throw error;
  }
}

const cliDocs = await createYarnCliDocs();

// eslint-disable-next-line arca/no-default-export
export default function yarnCliDocs() {
  return {
    name: `yarn-cli-docs-integration`,
    hooks: {
      "astro:server:setup": () => {
        return cliDocs;
      },
    },
  };
}
