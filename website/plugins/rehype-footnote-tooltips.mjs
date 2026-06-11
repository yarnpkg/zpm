import {visit, SKIP} from 'unist-util-visit';

function escAttr(s) {
  return s.replace(/&/g, `&amp;`).replace(/"/g, `&quot;`).replace(/</g, `&lt;`).replace(/>/g, `&gt;`);
}

function escText(s) {
  return s.replace(/&/g, `&amp;`).replace(/</g, `&lt;`).replace(/>/g, `&gt;`);
}

const VOID = new Set([`br`, `hr`, `img`, `input`]);

function serializeNode(node) {
  if (node.type === `text`)
    return escText(node.value);

  if (node.type === `element`) {
    const props = node.properties || {};

    if (props.dataFootnoteBackref != null)
      return ``;
    if (props.className?.includes(`data-footnote-backref`))
      return ``;

    const tag = node.tagName;

    const attrs = [];
    for (const [k, v] of Object.entries(props)) {
      if (k === `className`) {
        attrs.push(`class="${escAttr(v.join(` `))}"`);
      } else if (typeof v === `string`) {
        attrs.push(`${k.replace(/([A-Z])/g, `-$1`).toLowerCase()}="${escAttr(v)}"`);
      } else if (v === true) {
        attrs.push(k.replace(/([A-Z])/g, `-$1`).toLowerCase());
      }
    }

    const open = attrs.length ? `<${tag} ${attrs.join(` `)}>` : `<${tag}>`;
    if (VOID.has(tag))
      return open;

    const inner = (node.children || []).map(serializeNode).join(``);
    return `${open}${inner}</${tag}>`;
  }

  if (node.type === `raw`)
    return node.value;

  return ``;
}

function serializeFootnote(children) {
  return children
    .filter(c => c.type === `element` && c.tagName === `p`)
    .map(p => p.children.map(serializeNode).join(``).trim())
    .join(`<br>`)
    .trim();
}

export default function rehypeFootnoteTooltips() {
  return tree => {
    const footnotes = new Map();

    visit(tree, `element`, node => {
      if (node.tagName !== `li`)
        return;

      const id = node.properties?.id;
      if (!id || !id.startsWith(`user-content-fn-`))
        return;


      const key = id.replace(`user-content-fn-`, ``);
      const html = serializeFootnote(node.children);
      if (html) {
        footnotes.set(key, html);
      }
    });

    if (!footnotes.size) return;

    visit(tree, `element`, node => {
      if (node.tagName !== `sup`)
        return undefined;


      const link = (node.children || []).find(c =>
        c.type === `element` && c.tagName === `a` && c.properties?.dataFootnoteRef != null,
      );
      if (!link)
        return undefined;


      const key = (link.properties.href || ``).replace(`#user-content-fn-`, ``);
      const html = footnotes.get(key);
      if (!html)
        return undefined;


      node.properties = node.properties || {};
      node.properties.className = [...(node.properties.className || []), `fn-ref`];

      node.children.push({
        type: `raw`,
        value: `<span class="fn-tooltip">${html}</span>`,
      });

      return SKIP;
    });
  };
}
