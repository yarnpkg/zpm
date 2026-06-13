import {createHighlighter, createCssVariablesTheme} from 'shiki';
import {visit}                                      from 'unist-util-visit';

const cssVarsTheme = createCssVariablesTheme({
  name: `css-variables`,
  variablePrefix: `--shiki-`,
  variableDefaults: {},
  fontStyle: true,
});

function escapeHtml(str) {
  return str
    .replace(/&/g, `&amp;`)
    .replace(/</g, `&lt;`)
    .replace(/>/g, `&gt;`)
    .replace(/"/g, `&quot;`);
}

function toString(node) {
  if (node.type === `text`) return node.value;
  if (node.children) return node.children.map(toString).join(``);
  return ``;
}

function slugify(s) {
  return s.toLowerCase()
    .replace(/[^\w\s-]/g, ``)
    .replace(/\s+/g, `-`)
    .replace(/-+/g, `-`)
    .replace(/^-|-$/g, ``);
}

const LANG_ALIASES = {js: `javascript`, ts: `typescript`, sh: `shell`};

let _hlPromise;
function getHighlighter() {
  if (!_hlPromise) {
    _hlPromise = createHighlighter({
      themes: [cssVarsTheme],
      langs: [`javascript`, `typescript`, `json`, `yaml`, `bash`, `html`, `css`, `jsx`, `tsx`, `diff`, `shell`],
    });
  }
  return _hlPromise;
}

async function highlightCode(code, lang) {
  if (!lang) return escapeHtml(code);
  const resolved = LANG_ALIASES[lang] || lang;
  try {
    const hl = await getHighlighter();
    if (!hl.getLoadedLanguages().includes(resolved)) return escapeHtml(code);
    const html = hl.codeToHtml(code, {lang: resolved, theme: `css-variables`});
    const match = html.match(/<code>(.+?)<\/code>/s);
    return match ? match[1] : escapeHtml(code);
  } catch {
    return escapeHtml(code);
  }
}

const PILL_NAMES = [`type`, `required`, `since`, `default`, `deprecated`];

const PILL = `inline-flex items-center font-mono text-[11px] leading-none px-[7px] py-1 rounded-[5px] border tracking-[0.01em] whitespace-nowrap`;
const PILL_V = {
  type: `border-[var(--pill-type-border)] bg-[var(--pill-type-bg)] text-[var(--pill-type-fg)]`,
  required: `border-[var(--pill-req-border)] bg-[var(--pill-req-bg)] text-[var(--pill-req-fg)]`,
  since: `border-[var(--accent-line)] bg-[var(--accent-soft)] text-[var(--accent)]`,
  default: `border-[var(--line)] bg-[color-mix(in_oklch,var(--fg)_5%,transparent)] text-[var(--fg-dim)]`,
  deprecated: `border-[var(--line)] bg-[color-mix(in_oklch,var(--fg)_5%,transparent)] text-[var(--fg-mute)] line-through decoration-[var(--pill-dep-strike)] decoration-1`,
};

const CLS = {
  fieldHead: `flex flex-wrap items-center gap-2.5 mb-2.5 scroll-mt-[88px]`,
  fieldName: `font-mono text-[15.5px] font-medium text-[var(--fg)] tracking-[-0.005em]`,
  fieldAnchor: `field-anchor text-[var(--fg-mute)] no-underline font-normal transition-color duration-150 cursor-pointer select-none font-mono text-[15px] border-0 -ml-1 hover:text-[var(--accent)]`,
  fieldList: `border-t border-[var(--line-strong)] mt-0`,
};

function pillToHtml(name, content) {
  const cls = `${PILL} ${PILL_V[name] || PILL_V.default}`;
  switch (name) {
    case `type`: return `<span class="${cls}">${content}</span>`;
    case `required`: return `<span class="${cls}">required</span>`;
    case `since`: return `<span class="${cls}">${content}</span>`;
    case `default`: return `<span class="${cls}"><span class="text-[var(--fg-mute)] mr-1 font-normal">default:</span><span class="text-[var(--fg-dim)]">${content}</span></span>`;
    case `deprecated`: return `<span class="${cls}">${content}</span>`;
    default: return ``;
  }
}

function buildTerminalHtml(content) {
  const lines = content.split(`\n`);
  const spans = lines.map(line => {
    if (line.startsWith(`# `)) {
      return `<span class="term-line comment">${escapeHtml(line.slice(2))}</span>`;
    } else if (line.startsWith(`> `)) {
      return `<span class="term-line out">${escapeHtml(line.slice(2))}</span>`;
    } else {
      return `<span class="term-line">${escapeHtml(line)}</span>`;
    }
  }).join(`\n`);

  return `<div class="terminal">\n${spans}\n</div>`;
}

async function buildCodeBlockHtml(content, lang, title) {
  const highlighted = await highlightCode(content, lang);
  let html = `<div class="code-block">`;
  if (title) html += `\n<span class="code-lang">${escapeHtml(title)}</span>`;
  html += `\n<pre><code>${highlighted}</code></pre>\n</div>`;
  return html;
}

function isFieldHeading(node) {
  if (node.type !== `heading`) return false;
  const meaningful = node.children.filter(c => !(c.type === `text` && !c.value.trim()));
  if (!meaningful.length) return false;
  if (meaningful[0].type !== `inlineCode`) return false;
  return meaningful.slice(1).every(c => c.type === `textDirective` && PILL_NAMES.includes(c.name));
}

function processFieldHeadings(tree) {
  const children = tree.children;
  const newChildren = [];
  let i = 0;

  while (i < children.length) {
    if (isFieldHeading(children[i])) {
      const fieldDepth = children[i].depth;
      const fields = [];

      while (i < children.length) {
        if (!isFieldHeading(children[i])) break;

        const heading = children[i];
        const body = [];
        i++;

        while (i < children.length) {
          if (isFieldHeading(children[i])) break;
          if (children[i].type === `heading` && children[i].depth <= fieldDepth) break;
          body.push(children[i]);
          i++;
        }

        fields.push({heading, body});
      }

      for (const field of fields) {
        const inlineCode = field.heading.children.find(c => c.type === `inlineCode`);
        const name = inlineCode?.value || ``;
        const directives = field.heading.children.filter(c => c.type === `textDirective`);
        const pillsHtml = directives.map(d => pillToHtml(d.name, toString(d))).join(``);
        const id = `field-${slugify(name)}`;

        const nameHtml = `<span class="${CLS.fieldName}">${escapeHtml(name)}</span>`;
        const anchorHtml = `<a href="#${id}" class="${CLS.fieldAnchor}" aria-label="Copy link to this field">#</a>`;

        newChildren.push(
          {type: `html`, value: `<div id="${id}" class="${CLS.fieldHead}">${anchorHtml}${nameHtml}${pillsHtml}</div>`},
          ...field.body,
        );
      }
    } else {
      newChildren.push(children[i]);
      i++;
    }
  }

  tree.children = newChildren;
}

export default function remarkDocs() {
  return async tree => {
    const codeNodes = [];
    visit(tree, `code`, (node, index, parent) => {
      if (!parent) return;

      if (node.lang === `terminal`) {
        parent.children[index] = {
          type: `html`,
          value: buildTerminalHtml(node.value),
        };
        return;
      }

      codeNodes.push({node, index, parent});
    });

    await Promise.all(codeNodes.map(async ({node, index, parent}) => {
      const titleMatch = node.meta?.match(/title="([^"]+)"/);
      parent.children[index] = {
        type: `html`,
        value: await buildCodeBlockHtml(node.value, node.lang || ``, titleMatch?.[1] || ``),
      };
    }));

    visit(tree, `containerDirective`, (node, index, parent) => {
      if (!parent) return;
      const type = node.name;

      if ([`note`, `tip`, `warning`, `danger`].includes(type)) {
        const labelChild = node.children.find(c => c.data?.directiveLabel);
        const label = labelChild ? toString(labelChild) : type.toUpperCase();

        node.children = node.children.filter(c => !c.data?.directiveLabel);

        const data = node.data || (node.data = {});
        data.hName = `div`;
        data.hProperties = {
          className: [`admonition`, type],
          dataAdmonition: type,
          dataLabel: label,
        };
      }

      if (type === `steps`) {
        const ol = node.children.find(c => c.type === `list` && c.ordered);
        if (ol) {
          const data = ol.data || (ol.data = {});
          data.hProperties = {...(data.hProperties || {}), className: [`steps`]};
          parent.children[index] = ol;
          return;
        }
      }
    });

    visit(tree, `list`, node => {
      if (!node.ordered) return;
      for (const item of node.children) {
        if (item.type !== `listItem`) continue;
        item.children = [
          {type: `html`, value: `<div class="item">`},
          ...item.children,
          {type: `html`, value: `</div>`},
        ];
      }
    });

    processFieldHeadings(tree);

    visit(tree, `inlineCode`, (node, index, parent) => {
      if (!parent || parent.type === `link`) return;
      const match = node.value.match(/^([a-z]+):$/);
      if (!match) return;
      parent.children[index] = {
        type: `link`,
        url: `/protocol/${match[1]}.html`,
        children: [{type: `inlineCode`, value: node.value}],
      };
    });

    visit(tree, `textDirective`, (node, index, parent) => {
      if (!parent || !PILL_NAMES.includes(node.name)) return;
      const content = toString(node);
      parent.children[index] = {type: `html`, value: pillToHtml(node.name, content)};
      return;
    });
  };
}
