import {visit} from 'unist-util-visit';

const admonitionSvgs = {
  note: `<svg viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="7" cy="7" r="5.5"/><path d="M7 6.5v3.5M7 4v0.5"/></svg>`,
  tip: `<svg viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M7 1v1M7 12v1M1 7h1M12 7h1M3 3l.7.7M10.3 10.3l.7.7M3 11l.7-.7M10.3 3.7l.7-.7"/><circle cx="7" cy="7" r="2.5"/></svg>`,
  warning: `<svg viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M7 2L13 12H1L7 2Z"/><path d="M7 6v3M7 10.5v0.3"/></svg>`,
  danger: `<svg viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="7" cy="7" r="5.5"/><path d="M7 4v4M7 9.5v0.3"/></svg>`,
};


export default function rehypeDocs() {
  return tree => {
    visit(tree, `element`, node => {
      const type = node.properties?.dataAdmonition;
      if (!type || !admonitionSvgs[type]) return;

      const label = node.properties.dataLabel || type.toUpperCase();

      const header = {
        type: `element`,
        tagName: `div`,
        properties: {className: [`adm-header`]},
        children: [
          {type: `raw`, value: admonitionSvgs[type]},
          {
            type: `element`,
            tagName: `span`,
            properties: {},
            children: [{type: `text`, value: label}],
          },
        ],
      };

      const body = {
        type: `element`,
        tagName: `div`,
        properties: {className: [`adm-body`]},
        children: node.children,
      };

      node.children = [header, body];
      delete node.properties.dataAdmonition;
      delete node.properties.dataLabel;
    });

    // Heading anchors: wrap h2-h4 content and append # link
    function textContent(node) {
      if (node.type === `text`) return node.value;
      if (node.children) return node.children.map(textContent).join(``);
      return ``;
    }
    function slugifyId(s) {
      return s.toLowerCase()
        .replace(/[^\w\s-]/g, ``)
        .replace(/\s+/g, `-`)
        .replace(/-+/g, `-`)
        .replace(/^-|-$/g, ``);
    }

    visit(tree, `element`, node => {
      if (![`h2`, `h3`, `h4`].includes(node.tagName)) return;

      if (!node.properties.id)
        node.properties.id = slugifyId(textContent(node));


      const id = node.properties.id;
      const text = {type: `element`, tagName: `span`, properties: {}, children: node.children};
      const anchor = {
        type: `element`,
        tagName: `a`,
        properties: {
          href: `#${id}`,
          className: [`heading-anchor`],
          ariaLabel: `Copy link to this section`,
        },
        children: [{type: `text`, value: `#`}],
      };
      const wrap = {
        type: `element`,
        tagName: `span`,
        properties: {className: [`heading-wrap`]},
        children: [text, anchor],
      };
      node.children = [wrap];
    });

    // Lead paragraph: add .lead to the first <p> after <h1>
    const children = tree.children || [];
    for (let i = 0; i < children.length; i++) {
      const child = children[i];
      if (child.type === `element` && child.tagName === `h1`) {
        for (let j = i + 1; j < children.length; j++) {
          const next = children[j];
          if (next.type === `text` && !next.value.trim()) continue;
          if (next.type === `element` && next.tagName === `p`) {
            next.properties = next.properties || {};
            next.properties.className = [
              ...(next.properties.className || []),
              `lead`,
            ];
          }
          break;
        }
        break;
      }
    }
  };
}
