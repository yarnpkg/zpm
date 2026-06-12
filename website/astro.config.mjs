import react                from '@astrojs/react';
import sitemap              from '@astrojs/sitemap';
import tailwindcss          from '@tailwindcss/vite';
import {defineConfig}       from 'astro/config';
import remarkDirective      from 'remark-directive';

import rehypeDocs               from './plugins/rehype-docs.mjs';
import rehypeFootnoteTooltips   from './plugins/rehype-footnote-tooltips.mjs';
import remarkAutolinkFields from './plugins/remark-autolink-fields.mjs';
import remarkBluesky        from './plugins/remark-bluesky.mjs';
import remarkDocs           from './plugins/remark-docs.mjs';
import remarkMermaid        from './plugins/remark-mermaid.mjs';

const browserPodHeaders = {
  'Cross-Origin-Embedder-Policy': `require-corp`,
  'Cross-Origin-Opener-Policy': `same-origin`,
};

function browserPodPreviewHeaders() {
  const applyHeaders = (server) => {
    server.middlewares.use((req, res, next) => {
      if (req.url?.startsWith(`/playground`)) {
        for (const [name, value] of Object.entries(browserPodHeaders))
          res.setHeader(name, value);
      }

      next();
    });
  };

  return {
    name: `browserpod-preview-headers`,
    configurePreviewServer: applyHeaders,
    configureServer: applyHeaders,
  };
}

export default defineConfig({
  site: `https://v6.yarnpkg.com`,
  integrations: [react(), sitemap({filter: page => !page.includes(`/presentation/`)})],
  build: {
    format: `file`,
  },
  vite: {
    plugins: [tailwindcss(), browserPodPreviewHeaders()],
    optimizeDeps: {
      include: [
        `prettier/standalone`,
        `prettier/plugins/babel`,
        `prettier/plugins/estree`,
        `prettier/plugins/typescript`,
        `prettier/plugins/postcss`,
        `prettier/plugins/html`,
        `prettier/plugins/markdown`,
        `prettier/plugins/yaml`,
      ],
    },
  },
  markdown: {
    syntaxHighlight: false,
    remarkPlugins: [remarkDirective, remarkBluesky, remarkMermaid, remarkDocs, remarkAutolinkFields],
    rehypePlugins: [rehypeDocs, rehypeFootnoteTooltips],
  },
});
