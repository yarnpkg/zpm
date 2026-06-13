import {clipanionLoaders}    from '@clipanion/astro';
import {glob}                from 'astro/loaders';
import {defineCollection, z} from 'astro:content';
import path                  from 'path';
import {fileURLToPath}       from 'url';

import {cliBody}             from './utils/cli';

const docs = defineCollection({
  loader: glob({pattern: `**/*.{md,mdx}`, base: `./src/docs`}),
  schema: z.object({
    slug: z.string(),
    title: z.string(),
    description: z.string().optional(),
    category: z.string().optional(),
    sidebar_position: z.number().optional(),
    sidebar: z.object({
      order: z.number().optional(),
      hidden: z.coerce.boolean().optional(),
    }).optional(),
  }),
});

const blog = defineCollection({
  loader: glob({pattern: `**/*.{md,mdx}`, base: `./src/blog`}),
  schema: z.object({
    slug: z.string(),
    title: z.string(),
    date: z.coerce.date(),
    category: z.string().optional(),
    author: z.object({
      name: z.string(),
      title: z.string(),
      url: z.string().optional(),
      image_url: z.string().optional(),
    }),
  }),
});

const __dirname = fileURLToPath(new URL(`.`, import.meta.url));

const cliSchema = z.object({
  binaryName: z.string(),
  commandSpec: z.any(),
  title: z.string(),
  category: z.string(),
});

const cliLoaders = clipanionLoaders({
  id: `cli`,
  name: `yarn`,
  path: path.resolve(__dirname, `../../target/release/yarn-bin`),

  filter: entry => entry.data.commandSpec.category !== null,

  entry: entry => ({
    ...entry,
    data: {
      ...entry.data,
      title: `yarn ${entry.data.commandSpec.primaryPath.join(` `)}`,
      category: entry.data.commandSpec.category!,
    },
  }),

  body: cliBody,
});

const switchLoaders = clipanionLoaders({
  id: `switch`,
  name: `yarn`,
  path: path.resolve(__dirname, `../../target/release/yarn`),

  specCommand: [`switch`, `--clipanion-commands`],

  filter: entry =>
    entry.data.commandSpec.category !== null
    && entry.data.commandSpec.primaryPath[0] === `switch`,

  entry: entry => ({
    ...entry,
    data: {
      ...entry.data,
      title: `yarn ${entry.data.commandSpec.primaryPath.join(` `)}`,
      category: entry.data.commandSpec.category!,
    },
  }),

  body: cliBody,
});

export const collections = {
  blog,
  docs,
  cli: defineCollection({loader: cliLoaders.commands, schema: cliSchema}),
  switch: defineCollection({loader: switchLoaders.commands, schema: cliSchema}),
};
