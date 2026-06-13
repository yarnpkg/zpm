import {createRequire}    from 'module';
import {visit}            from 'unist-util-visit';

import {schemaFieldNames} from '../src/utils/schema.ts';

const require = createRequire(import.meta.url);

function slugify(s) {
  return s.toLowerCase()
    .replace(/[^\w\s-]/g, ``)
    .replace(/\s+/g, `-`)
    .replace(/-+/g, `-`)
    .replace(/^-|-$/g, ``);
}

function buildFieldMap() {
  const manifest = require(`../config/manifest.json`);
  const yarnrc = require(`../config/yarnrc.json`);

  const map = new Map();

  for (const name of schemaFieldNames(manifest))
    map.set(name, {url: `/configuration/manifest.html`, anchor: `field-${slugify(name)}`});


  for (const name of schemaFieldNames(yarnrc))
    if (!map.has(name))
      map.set(name, {url: `/configuration/yarnrc.html`, anchor: `field-${slugify(name)}`});


  map.set(`package.json`, {url: `/configuration/manifest.html`, anchor: null});
  map.set(`.yarnrc.yml`, {url: `/configuration/yarnrc.html`, anchor: null});

  return map;
}

export default function remarkAutolinkFields() {
  const fieldMap = buildFieldMap();

  return tree => {
    const replacements = [];

    visit(tree, `inlineCode`, (node, index, parent) => {
      if (!parent || index === undefined)
        return;

      if (parent.type === `heading` || parent.type === `link`)
        return;

      const target = fieldMap.get(node.value);
      if (!target)
        return;

      replacements.push({parent, index, node, target});
    });

    for (let i = replacements.length - 1; i >= 0; i--) {
      const {parent, index, node, target} = replacements[i];
      parent.children.splice(index, 1, {
        type: `link`,
        url: target.anchor ? `${target.url}#${target.anchor}` : target.url,
        children: [node],
      });
    }
  };
}
