import assert from 'node:assert/strict';
import {test} from 'node:test';

import {schemaToMarkdown} from '../src/utils/schema.ts';

test('renders nullable enums and scalar examples', () => {
  const markdown = schemaToMarkdown({properties: {
    access: {type: [`string`, `null`], enum: [`public`, `restricted`, null], examples: [`public`, null]},
  }});

  assert.ok(markdown.includes(':type["public" | "restricted" | null]'));
  assert.ok(markdown.includes('access: "public"'));
  assert.ok(markdown.includes('access: null'));
});

test('renders scalar, array, and null architecture forms', () => {
  const markdown = schemaToMarkdown({properties: {
    cpu: {type: [`string`, `array`, `null`], items: {type: `string`}},
    targets: {type: `array`, items: {type: [`string`, `null`]}},
  }});

  assert.ok(markdown.includes(':type[string | string\\[\\] | null]'));
  assert.ok(markdown.includes(':type[(string | null)\\[\\]]'));
});

test('preserves rich example descriptions and values', () => {
  const markdown = schemaToMarkdown({properties: {
    cpu: {
      type: [`array`, `null`], items: {type: `string`}, examples: [null],
      _examples: [{description: `Cover both architectures.`, value: [`x64`, `arm64`]}],
    },
  }});

  assert.ok(markdown.includes('# Cover both architectures.'));
  assert.ok(markdown.includes('cpu:\n  - "x64"\n  - "arm64"'));
  assert.ok(!markdown.includes('cpu: null'));
});
