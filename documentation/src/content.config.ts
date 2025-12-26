import {docsLoader}                                                  from '@astrojs/starlight/loaders';
import {docsSchema}                                                  from '@astrojs/starlight/schema';
import {clipanionLoaders, type BaseData}                             from '@clipanion/astro';
import {file, glob, type DataStore, type Loader, type LoaderContext} from 'astro/loaders';
import {defineCollection, reference, z}                              from 'astro:content';
import dedent                                                        from 'dedent';
import {createRequire}                                               from 'module';
import {autoSidebarLoader}                                           from 'starlight-auto-sidebar/loader';
import {autoSidebarSchema}                                           from 'starlight-auto-sidebar/schema';

const require = createRequire(import.meta.url);

const clipanionBody = ({data: {binaryName, commandSpec}}: {data: {binaryName: string, commandSpec: BaseData[`commandSpec`], title: string}}) => {
  const options = commandSpec.components
    .filter((component): component is Extract<typeof component, {type: `option`}> => component.type === `option` && !component.isHidden);

  return dedent.withOptions({alignValues: true})`
    \`\`\`bash
    ${binaryName} ${commandSpec.primaryPath.join(` `)}
    \`\`\`

    ${commandSpec.documentation?.details}

    ${options.length > 0 ? dedent`
      ### Options

      <div class="[&_table]:table-fixed [&_th:first-child]:w-[250px]">

      | Option | Description |
      | --- | --- |
      ${options.map(option => dedent`
        | \`${option.primaryName}\` | ${option.documentation?.description} |
      `).join(`\n`)}

      </div>
    ` : ``}
  `;
};

const yarnCliLoaders = clipanionLoaders({
  id: `reference/cli`,

  name: `yarn`,
  path: require.resolve(`@yarnpkg/monorepo/target/release/yarn-bin`),

  filter: entry => {
    return entry.data.commandSpec.category !== null;
  },

  entry: entry => ({
    ...entry,
    filePath: `src/content/docs/reference/CLI reference/${entry.data.commandSpec.category}/${entry.data.commandSpec.primaryPath.join(`-`)}.md`,
    data: {
      ...entry.data,
      slug: `cli/${entry.data.commandSpec.primaryPath.join(`/`)}`,
      title: `${entry.data.binaryName} ${entry.data.commandSpec.primaryPath.join(` `)}`,
      draft: false,
      head: [],
      sidebar: {
        label: `${entry.data.binaryName} ${entry.data.commandSpec.primaryPath.join(` `)}`,
      },
    },
  }),

  body: clipanionBody,
});

const yarnSwitchCliLoaders = clipanionLoaders({
  id: `reference/switch`,

  name: `yarn`,
  path: require.resolve(`@yarnpkg/monorepo/target/release/yarn`),

  specCommand: [`switch`, `--clipanion-commands`],

  filter: entry => {
    return entry.data.commandSpec.category !== null && entry.data.commandSpec.primaryPath[0] === `switch`;
  },

  entry: entry => ({
    ...entry,
    filePath: `src/content/docs/reference/Yarn Switch/${entry.data.commandSpec.category}/${entry.data.commandSpec.primaryPath.join(`-`)}.md`,
    data: {
      ...entry.data,
      slug: `cli/${entry.data.commandSpec.primaryPath.join(`/`)}`,
      title: `${entry.data.binaryName} ${entry.data.commandSpec.primaryPath.join(` `)}`,
      draft: false,
      head: [],
      sidebar: {
        label: `${entry.data.binaryName} ${entry.data.commandSpec.primaryPath.join(` `)}`,
      },
    },
  }),

  body: clipanionBody,
});

export const collections = {
  docs: defineCollection({
    loader: unionLoader(docsLoader(), yarnCliLoaders.commands, yarnSwitchCliLoaders.commands),
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

export function unionLoader(...loaders: Array<Loader>): Loader {
  const names = loaders.map(loader => loader.name);
  const schemas = loaders.flatMap(loader => loader.schema);

  return {
    name: names.join(` + `),

    schema: async () => {
      const awaitedSchemas = await Promise.all(schemas.map(schema => (typeof schema === `function` ? schema() : schema)));
      const filteredSchemas = awaitedSchemas.filter(schema => schema !== undefined);

      return z.union(filteredSchemas as any);
    },

    load: async ctx => {
      function createContext(ctx: LoaderContext): LoaderContext {
        const entries = new Set<string>();

        const subStore: DataStore = {
          get: id => ctx.store.get(id),

          entries: () => {
            return Array.from(entries).map(id => [id, ctx.store.get(id)!]);
          },

          set: (opts: any) => {
            entries.add(opts.id);
            return ctx.store.set(opts);
          },

          values: () => {
            return Array.from(entries).map(id => ctx.store.get(id)!);
          },

          keys: () => {
            return Array.from(entries);
          },

          delete: (id: string) => {
            entries.delete(id);
            return ctx.store.delete(id);
          },

          clear: () => {
            for (const entry of entries)
              ctx.store.delete(entry);

            entries.clear();
          },

          has: (id: string) => {
            return entries.has(id);
          },

          addModuleImport: (fileName: string) => {
            return ctx.store.addModuleImport(fileName);
          },
        };

        return {
          ...ctx,
          store: subStore,
        };
      }

      await Promise.all(loaders.map(loader => loader.load(createContext(ctx))));
    },
  };
}
