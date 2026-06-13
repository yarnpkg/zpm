import rehypeRaw       from 'rehype-raw';
import rehypeStringify from 'rehype-stringify';
import remarkDirective from 'remark-directive';
import remarkParse     from 'remark-parse';
import remarkRehype    from 'remark-rehype';
import {unified}       from 'unified';

import rehypeDocs      from '../../plugins/rehype-docs.mjs';
import remarkDocs      from '../../plugins/remark-docs.mjs';

const processor = unified()
  .use(remarkParse)
  .use(remarkDirective)
  .use(remarkDocs)
  .use(remarkRehype, {allowDangerousHtml: true})
  .use(rehypeRaw)
  .use(rehypeDocs)
  .use(rehypeStringify);

export async function renderDocsMarkdown(md: string): Promise<string> {
  const result = await processor.process(md);
  return String(result);
}
