export function titleToSectionTag(title: string): string {
  return title
    .replace(/<br\s*\/?>/gi, ` `)
    .replace(/<[^>]+>/g, ``)
    .replace(/&nbsp;/g, ` `)
    .replace(/&apos;/g, `'`)
    .replace(/&amp;/g, `&`)
    .replace(/\s+/g, ` `)
    .trim();
}
