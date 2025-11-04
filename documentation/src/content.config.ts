import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';
import { file, glob, type Loader } from 'astro/loaders';
import { defineCollection, reference, z } from 'astro:content';
import dedent from 'dedent';
import { autoSidebarLoader } from 'starlight-auto-sidebar/loader';
import { autoSidebarSchema } from 'starlight-auto-sidebar/schema';

import { clipanionLoaders } from '@clipanion/astro';
import { createRequire } from 'module';

const require = createRequire(import.meta.url);

const yarnCliLoaders = clipanionLoaders({
  id: `reference/cli`,

  name: `yarn`,
  path: require.resolve(`@yarnpkg/monorepo/target/release/yarn-bin`),

  entry: entry => ({
    ...entry,
    filePath: `src/content/docs/reference/${entry.data.commandSpec.category}/${entry.data.commandSpec.primaryPath.join(`-`)}.md`,
    data: {
      ...entry.data,
      slug: `cli/${entry.data.commandSpec.primaryPath.join(`/`)}`,
      title: `${entry.data.binaryName} ${entry.data.commandSpec.primaryPath.join(` `)}`,
      head: [],
      sidebar: {
        label: `${entry.data.binaryName} ${entry.data.commandSpec.primaryPath.join(` `)}`,
      },
    },
  }),

  body: ({ data: { binaryName, commandSpec, title } }) => {
    const options = commandSpec.components
      .filter((component): component is Extract<typeof component, { type: 'option' }> => component.type === `option` && !component.isHidden);

    return dedent.withOptions({ alignValues: true })`
      ## ${title}

      ${commandSpec.documentation?.description}

      \`\`\`bash
      ${binaryName} ${commandSpec.primaryPath.join(` `)}
      \`\`\`

      ${commandSpec.documentation?.details}

      ${options.length > 0 ? dedent`
        ### Options

        <div class="[&_table]:table-fixed [&_th:first-child]:w-[200px]">

        | Option | Description |
        | --- | --- |
        ${options.map(option => dedent`
          | \`${option.primaryName}\` | ${option.documentation?.description} |
        `).join(`\n`)}

        </div>
      `: ``}
    `;
  },
});

export const collections = {
  docs: defineCollection({
    loader: unionLoader(docsLoader(), yarnCliLoaders.commands),
    schema: docsSchema(),
  }),

  blog: defineCollection({
    loader: glob({
      pattern: `**/*.mdx`,
      base: `./src/content/blog`,
    }),
    schema: z.object({
      title: z.string(),
      slug: z.string(),
      author: reference(`authors`),
      description: z
        .string()
        .optional()
        .transform(desc => desc?.trim() || ``),
    }),
  }),

  autoSidebar: defineCollection({
    loader: autoSidebarLoader(),
    schema: autoSidebarSchema(),
  }),

  authors: defineCollection({
    loader: file(`src/content/blog/authors.yml`),
    schema: z.object({
      id: z.string(),
      name: z.string(),
      title: z.string().optional(),
      url: z.string().url().optional(),
      image_url: z.string().url().optional(),
      socials: z
        .object({
          mastodon: z.string().optional(),
          linkedin: z.string().optional(),
          bluesky: z.string().optional(),
          github: z.string().optional(),
          website: z.string().url().optional(),
        })
        .optional(),
    }),
  }),
};

export function unionLoader(...loaders: Loader[]): Loader {
  const names = loaders.map((loader) => loader.name);
  const schemas = loaders.flatMap((loader) => loader.schema);

  return {
    name: names.join(' + '),

    schema: async () => {
      const awaitedSchemas = await Promise.all(schemas.map((schema) => (typeof schema === `function` ? schema() : schema)));
      const filteredSchemas = awaitedSchemas.filter((schema) => schema !== undefined);

      return z.union([filteredSchemas[0], filteredSchemas[1], ...filteredSchemas.slice(2)]);
    },

    load: async (ctx) => {
      await Promise.all(loaders.map((loader) => loader.load(ctx)));
    },
  };
}
