import rehypeRaw              from 'rehype-raw';
import rehypeStringify        from 'rehype-stringify';
import remarkGfm              from 'remark-gfm';
import remarkParse            from 'remark-parse';
import remarkRehype           from 'remark-rehype';
import {unified}              from 'unified';

import rehypeFootnoteTooltips from './rehype-footnote-tooltips.mjs';

async function render(md) {
  const file = await unified()
    .use(remarkParse)
    .use(remarkGfm)
    .use(remarkRehype, {allowDangerousHtml: true})
    .use(rehypeRaw)
    .use(rehypeFootnoteTooltips)
    .use(rehypeStringify, {allowDangerousHtml: true})
    .process(md);

  return String(file);
}

let passed = 0;
let failed = 0;

function assert(condition, label) {
  if (condition) {
    passed++;
    console.log(`  PASS  ${label}`);
  } else {
    failed++;
    console.log(`  FAIL  ${label}`);
  }
}

function getTooltipContent(html) {
  const m = html.match(/<span class="fn-tooltip">([\s\S]*?)<\/span>/);
  return m ? m[1] : null;
}

// ── Test 1: basic footnote ──

const basic = await render(`
Hello world[^1].

[^1]: This is a footnote.
`);

console.log(`\n─── Test 1: basic footnote ───`);
console.log(basic, `\n`);

assert(basic.includes(`fn-ref`), `sup gets fn-ref class`);
assert(basic.includes(`fn-tooltip`), `tooltip element exists`);
assert(!basic.includes(`<script`), `no script tag`);

const fnRefMatch = basic.match(/<sup[^>]*class="[^"]*fn-ref[^"]*"[^>]*>([\s\S]*?)<\/sup>/);
assert(fnRefMatch !== null, `fn-ref sup found`);
if (fnRefMatch) {
  assert(fnRefMatch[1].includes(`fn-tooltip`), `tooltip is inside fn-ref`);
  assert(fnRefMatch[1].includes(`This is a footnote`), `content inside fn-ref`);
}

const content1 = getTooltipContent(basic);
assert(content1 !== null && !content1.includes(`<p>`), `no <p> in tooltip`);
assert(content1 !== null && content1.includes(`This is a footnote`), `text content present`);

// ── Test 2: footnotes section stays visible ──

console.log(`─── Test 2: footnotes section visible ───`);

assert(!basic.includes(`class="footnotes sr-only"`), `no sr-only on footnotes section`);
assert(basic.includes(`data-footnotes`), `footnotes section present`);

// ── Test 3: click-through links work ──

console.log(`─── Test 3: click-through links ───`);

const refLink = basic.match(/<a[^>]*href="(#user-content-fn-[^"]*)"[^>]*data-footnote-ref/);
assert(refLink !== null, `footnote ref link exists`);
if (refLink) {
  const targetId = refLink[1].replace(`#`, ``);
  assert(basic.includes(`id="${targetId}"`), `link target exists in footnotes section`);
}

const backrefLink = basic.match(/<a[^>]*href="(#user-content-fnref-[^"]*)"[^>]*data-footnote-backref/);
assert(backrefLink !== null, `backref link exists in footnotes section`);
if (backrefLink) {
  const backTargetId = backrefLink[1].replace(`#`, ``);
  assert(basic.includes(`id="${backTargetId}"`), `backref target exists`);
}

// ── Test 4: multiple footnotes ──

const multi = await render(`
First[^a] and second[^b].

[^a]: Alpha note.
[^b]: Beta note.
`);

console.log(`\n─── Test 4: multiple footnotes ───`);
console.log(multi, `\n`);

assert(multi.includes(`Alpha note`), `first footnote content`);
assert(multi.includes(`Beta note`), `second footnote content`);

const refMatches = multi.match(/<sup[^>]*class="[^"]*fn-ref[^"]*"[^>]*>/g);
assert(refMatches && refMatches.length === 2, `two fn-ref elements`);

// ── Test 5: rich content ──

const rich = await render(`
Text[^1].

[^1]: Contains **bold**, \`code\`, and [a link](https://example.com).
`);

console.log(`─── Test 5: rich content ───`);
console.log(rich, `\n`);

assert(rich.includes(`<strong>bold</strong>`), `bold in tooltip`);
assert(rich.includes(`<code>code</code>`), `code in tooltip`);
assert(rich.includes(`href="https://example.com"`), `link in tooltip`);

const content5 = getTooltipContent(rich);
assert(content5 !== null && !content5.includes(`<p>`), `no <p> in rich tooltip`);

// ── Test 6: no footnotes = no transformation ──

const noFn = await render(`
Just a regular paragraph.
`);

console.log(`─── Test 6: no footnotes ───`);

assert(!noFn.includes(`fn-ref`), `no fn-ref`);
assert(!noFn.includes(`fn-tooltip`), `no tooltip`);

// ── Test 7: backref stripped from tooltip ──

console.log(`─── Test 7: backref stripped from tooltip ───`);

const content7 = getTooltipContent(basic);
if (content7) {
  assert(!content7.includes(`data-footnote-backref`), `backref removed from tooltip`);
  assert(!content7.includes(`↩`), `backref arrow removed from tooltip`);
} else {
  assert(false, `could not find tooltip content`);
  assert(false, `(skipped backref arrow check)`);
}

// ── Test 8: multi-paragraph footnote ──

const multiPara = await render(`
Text[^1].

[^1]: First paragraph.

    Second paragraph.
`);

console.log(`\n─── Test 8: multi-paragraph footnote ───`);
console.log(multiPara, `\n`);

const content8 = getTooltipContent(multiPara);
assert(content8 !== null && content8.includes(`First paragraph`), `first paragraph present`);
assert(content8 !== null && content8.includes(`Second paragraph`), `second paragraph present`);
assert(content8 !== null && content8.includes(`<br>`), `paragraphs separated by <br>`);
assert(content8 !== null && !content8.includes(`<p>`), `no <p> in multi-paragraph`);

// ── Summary ──

console.log(`${`═`.repeat(40)}`);
console.log(`  ${passed} passed, ${failed} failed`);
console.log(`${`═`.repeat(40)}\n`);

process.exit(failed > 0 ? 1 : 0);
