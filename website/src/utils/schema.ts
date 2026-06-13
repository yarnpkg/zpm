function escapeDirective(s: string): string {
  return s.replace(/\\/g, `\\\\`).replace(/\[/g, `\\[`).replace(/\]/g, `\\]`);
}

function formatType(prop: Record<string, any>): string {
  if (Array.isArray(prop.type))
    return prop.type.join(` | `);

  if (prop.enum)
    return prop.enum.map((v: any) => typeof v === `string` ? `"${v}"` : String(v)).join(` | `);

  if (prop.type === `array`)
    return `${prop.items?.type || `any`}[]`;

  return prop.type || `any`;
}

function scalarToYaml(value: any): string {
  if (typeof value === `string`)
    return JSON.stringify(value);

  if (typeof value === `number` || typeof value === `boolean`)
    return String(value);

  if (value === null)
    return `null`;

  return JSON.stringify(value);
}

function valueToYaml(value: any, indent = 0): string {
  const prefix = ` `.repeat(indent);

  if (Array.isArray(value)) {
    if (value.length === 0)
      return `[]`;

    return value.map(item => {
      if (item !== null && typeof item === `object`)
        return `${prefix}-\n${valueToYaml(item, indent + 2)}`;

      return `${prefix}- ${scalarToYaml(item)}`;
    }).join(`\n`);
  }

  if (value !== null && typeof value === `object`) {
    const entries = Object.entries(value);

    if (entries.length === 0)
      return `{}`;

    return entries.map(([key, item]) => {
      const formattedKey = /^[a-zA-Z0-9_-]+$/.test(key)
        ? key
        : JSON.stringify(key);

      if (item !== null && typeof item === `object`) {
        const nested = valueToYaml(item, indent + 2);

        if (Array.isArray(item) && item.length === 0)
          return `${prefix}${formattedKey}: []`;

        if (!Array.isArray(item) && Object.keys(item).length === 0)
          return `${prefix}${formattedKey}: {}`;

        return `${prefix}${formattedKey}:\n${nested}`;
      }

      return `${prefix}${formattedKey}: ${scalarToYaml(item)}`;
    }).join(`\n`);
  }

  return scalarToYaml(value);
}

function yamlComment(s: string): string {
  return s
    .split(/\r?\n/)
    .map(line => `# ${line}`)
    .join(`\n`);
}

function exampleToYaml(name: string, example: any): string {
  const description = example?.description || `Example`;
  const value = example !== null && typeof example === `object` && Object.hasOwn(example, `value`)
    ? example.value
    : example;

  return [
    yamlComment(description),
    valueToYaml({[name]: value}),
  ].join(`\n`);
}

function propertyToMarkdown(name: string, prop: Record<string, any>): string {
  const pills = [`:type[${escapeDirective(formatType(prop))}]`];

  if (prop.default !== undefined)
    pills.push(`:default[${escapeDirective(JSON.stringify(prop.default))}]`);

  const lines = [`### \`${name}\` ${pills.join(` `)}`];

  if (prop.title)
    lines.push(``, `**${prop.title}**`);


  if (prop.description)
    lines.push(``, prop.description);


  if (Array.isArray(prop._examples) && prop._examples.length > 0) {
    lines.push(
      ``,
      `\`\`\`yaml`,
      prop._examples.map((example: any) => exampleToYaml(name, example)).join(`\n\n`),
      `\`\`\``,
    );
  }

  return lines.join(`\n`);
}

function isHidden(prop: Record<string, any>): boolean {
  return prop._hidden === true;
}

function flattenToMarkdown(properties: Record<string, any>, prefix = ``): Array<string> {
  const sections: Array<string> = [];

  for (const [key, prop] of Object.entries(properties)) {
    if (isHidden(prop as Record<string, any>))
      continue;


    const name = prefix + key;
    sections.push(propertyToMarkdown(name, prop as Record<string, any>));

    if ((prop as any).properties)
      sections.push(...flattenToMarkdown((prop as any).properties, `${name}.`));


    if ((prop as any).patternProperties) {
      for (const patternProp of Object.values((prop as any).patternProperties) as Array<any>) {
        if (patternProp.properties) {
          sections.push(...flattenToMarkdown(patternProp.properties, `${name}[name].`));
        }
      }
    }
  }

  return sections;
}

export function schemaFieldNames(schema: Record<string, any>): Array<string> {
  return flattenFieldNames(schema.properties);
}

function flattenFieldNames(properties: Record<string, any>, prefix = ``): Array<string> {
  const names: Array<string> = [];

  for (const [key, prop] of Object.entries(properties)) {
    if (isHidden(prop as Record<string, any>))
      continue;


    const name = prefix + key;
    names.push(name);

    if ((prop as any).properties)
      names.push(...flattenFieldNames((prop as any).properties, `${name}.`));


    if ((prop as any).patternProperties) {
      for (const patternProp of Object.values((prop as any).patternProperties) as Array<any>) {
        if (patternProp.properties) {
          names.push(...flattenFieldNames(patternProp.properties, `${name}[name].`));
        }
      }
    }
  }

  return names;
}

export function schemaToMarkdown(schema: Record<string, any>): string {
  const parts: Array<string> = [];

  if (schema.description)
    parts.push(schema.description);

  parts.push(...flattenToMarkdown(schema.properties));

  return parts.join(`\n\n`);
}
