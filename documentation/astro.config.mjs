import preact                       from '@astrojs/preact';
import starlightDocSearch           from '@astrojs/starlight-docsearch';
import starlight                    from '@astrojs/starlight';
import tailwindcss                  from '@tailwindcss/vite';
// @ts-check
import {defineConfig}               from 'astro/config';
import starlightAutoSidebar         from 'starlight-auto-sidebar';
import svgr                         from 'vite-plugin-svgr';

import {remarkCommandLineHighlight} from './src/plugins/remark-command-line-highlight.mjs';
import {remarkModifiedTime}         from './src/plugins/remark-modified-time.mjs';
import {remarkReadingTime}          from './src/plugins/remark-reading-time.mjs';
import yarnCliDocs                  from './src/plugins/yarn-cli-docs';

// eslint-disable-next-line arca/no-default-export
export default defineConfig({
  output: `static`,
  prefetch: {
    prefetchAll: true,
  },
  integrations: [
    starlight({
      title: `Yarn`,
      head: [
        // Example: add Fathom analytics script tag.
        {
          tag: `meta`,
          attrs: {
            name: `robots`,
            content: `noindex`,
          },
        },
      ],
      social: [
        {
          icon: `discord`,
          label: `Discord`,
          href: `https://discord.com/invite/yarnpkg`,
        },
        {
          icon: `github`,
          label: `GitHub`,
          href: `https://github.com/yarnpkg/berry`,
        },
      ],
      sidebar: [
        {
          label: `Getting Started`,
          collapsed: true,
          autogenerate: {directory: `getting-started`},
        },
        {
          label: `CLI`,
          items: [
            {
              label: ``,
              collapsed: true,
              autogenerate: {directory: `cli/cli`},
            },
            {
              label: ``,
              autogenerate: {directory: `cli/builder`},
            },
            {
              label: ``,
              autogenerate: {directory: `cli/pnpify`},
            },
            {
              label: ``,
              autogenerate: {directory: `cli/sdks`},
            },
          ],
        },
        {
          label: `Advanced`,
          autogenerate: {directory: `advanced`},
        },
        {label: `Features`, autogenerate: {directory: `features`}},
        {
          label: `Configuration`,
          autogenerate: {directory: `configuration`},
        },
      ],
      components: {
        SocialIcons: `./src/overrides/CustomSocialIcons.astro`,
        Header: `./src/overrides/navigation/index.astro`,
        Sidebar: `./src/overrides/Sidebar.astro`,
        Search: `./src/overrides/CustomSearch.astro`,
        PageTitle: `./src/overrides/PageTitle.astro`,
        PageFrame: `./src/overrides/PageFrame.astro`,
        Pagination: `./src/overrides/Pagination.astro`,
        MarkdownContent: `./src/overrides/MarkdownContent.astro`,
        ContentPanel: `./src/overrides/ContentPanel.astro`,
        Head: `./src/overrides/Head.astro`,
      },
      customCss: [
        `./src/styles/global.css`,
        `@fontsource/montserrat/400.css`,
        `@fontsource/montserrat/500.css`,
        `./src/fonts/font-face.css`,
      ],
      plugins: [
        starlightAutoSidebar(),
        starlightDocSearch({
          appId: `STXW7VT1S5`,
          apiKey: `ecdfaea128fd901572b14543a2116eee`,
          indexName: `yarnpkg_next`,
        }),
      ],
      expressiveCode: {
        styleOverrides: {
          borderRadius: `16px`,
          borderWidth: `1.5px`,
          borderColor: `rgba(255, 255, 255, 0.05)`,
          codeFontSize: `16px`,
          uiPaddingBlock: `24px`,
          codePaddingInline: `24px`,
          codePaddingBlock: `16px`,
          frames: {
            terminalTitlebarBackground: `rgba(255, 255, 255, 0.03)`,
          },
        },
      },
      disable404Route: true,
      tableOfContents: false,
    }),
    preact({compat: true}),
    yarnCliDocs(),
  ],
  vite: {
    plugins: [tailwindcss(), svgr()],
    ssr: {
      noExternal: [`@base-ui-components/react/*`],
    },
    server: {
      fs: {
        allow: [
          process.env.CACHE_CWD,
          import.meta.dirname,
        ],
      },
    },
    define: {
      process: {
        env: {},
        version: `18.0.0`,
        versions: {node: `18.0.0`},
      },
    },
  },
  markdown: {
    remarkPlugins: [
      remarkReadingTime,
      remarkModifiedTime,
      remarkCommandLineHighlight,
    ],
  },
});
