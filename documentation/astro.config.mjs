import preact                       from '@astrojs/preact';
import starlightDocSearch           from '@astrojs/starlight-docsearch';
import starlight                    from '@astrojs/starlight';
import {clipanionRemark}            from '@clipanion/remark';
import tailwindcss                  from '@tailwindcss/vite';
import path from 'path';
// @ts-check
import {defineConfig}               from 'astro/config';
import starlightAutoSidebar         from 'starlight-auto-sidebar';
import svgr                         from 'vite-plugin-svgr';

import {remarkCommandLineHighlight} from './src/plugins/remark-command-line-highlight.mjs';
import {remarkModifiedTime}         from './src/plugins/remark-modified-time.mjs';
import {remarkReadingTime}          from './src/plugins/remark-reading-time.mjs';

// eslint-disable-next-line arca/no-default-export
export default defineConfig({
  site: process.env.DEPLOY_PRIME_URL !== `https://main--yarn6.netlify.app`
    ? process.env.DEPLOY_PRIME_URL ?? `https://yarnpkg.com`
    : `https://yarnpkg.com`,
  base: `/`,
  output: `static`,
  trailingSlash: `never`,
  prefetch: {
    prefetchAll: true,
  },
  integrations: [
    starlight({
      title: `Yarn`,
      head: [{
        tag: `meta`,
        attrs: {
          name: `robots`,
          content: `noindex`,
        },
      }],
      social: [{
        icon: `discord`,
        label: `Discord`,
        href: `https://discord.com/invite/yarnpkg`,
      }, {
        icon: `github`,
        label: `GitHub`,
        href: `https://github.com/yarnpkg/zpm`,
      }],
      sidebar: [{
        label: `Getting Started`,
        collapsed: true,
        autogenerate: {directory: `getting-started`},
      }, {
        label: `CLI`,
        items: [{
          label: ``,
          collapsed: true,
          autogenerate: {directory: `cli/cli`},
        }, {
          label: ``,
          autogenerate: {directory: `cli/builder`},
        }, {
          label: ``,
          autogenerate: {directory: `cli/pnpify`},
        }, {
          label: ``,
          autogenerate: {directory: `cli/sdks`},
        }],
      }, {
        label: `Concepts`,
        autogenerate: {directory: `concepts`},
      }, {
        label: `Appendix`,
        autogenerate: {directory: `appendix`},
      }, {
        label: `Contributing`,
        autogenerate: {directory: `contributing`},
      }, {
        label: `Reference`,
        autogenerate: {directory: `reference`},
      }],
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
        useStarlightDarkModeSwitch: false,
        useDarkModeMediaQuery: false,
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
            terminalTitlebarBorderBottomColor: `rgba(255, 255, 255, 0.05)`,
          },
        }
      },
      disable404Route: true,
      tableOfContents: false,
      // https://github.com/withastro/starlight/blob/b33473fc85be10a1f8fb53e1c35760bb54d23d11/packages/starlight/index.ts#L162
      pagefind: false,
    }),
    preact({compat: true}),
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
      [clipanionRemark, {
        clis: {
          yarn: {
            baseUrl: `https://example.org/git`,
            path: path.resolve(import.meta.dirname, `../target/release/yarn-bin`),
          },
        },
        enableBlocks: false,
      }],
    ],
  },
});
