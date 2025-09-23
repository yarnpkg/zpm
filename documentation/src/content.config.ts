import { defineCollection, reference, z } from "astro:content";
import { docsLoader } from "@astrojs/starlight/loaders";
import { docsSchema } from "@astrojs/starlight/schema";
import { file, glob } from "astro/loaders";
import { autoSidebarLoader } from "starlight-auto-sidebar/loader";
import { autoSidebarSchema } from "starlight-auto-sidebar/schema";

export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
  blog: defineCollection({
    loader: glob({ pattern: "**/*.mdx", base: "./src/content/blog" }),
    schema: z.object({
      title: z.string(),
      slug: z.string(),
      author: reference("authors"),
      description: z
        .string()
        .optional()
        .transform((desc) => desc?.trim() || ""),
    }),
  }),

  protocols: defineCollection({
    loader: glob({ pattern: "**/*.mdx", base: "./src/content/protocols" }),
    schema: z.object({
      title: z.string().optional(),
      slug: z.string().optional(),
    }),
  }),

  autoSidebar: defineCollection({
    loader: autoSidebarLoader(),
    schema: autoSidebarSchema(),
  }),

  authors: defineCollection({
    loader: file("src/content/blog/authors.yml"),
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
