import fs from "node:fs/promises";
import { mkdir, writeFile } from "node:fs/promises";

export function dedent(value: string): string {
  return value
    .trim()
    .split("\n")
    .map((line) => line.trimStart())
    .join("\n");
}

export function toHyphenCase(str: string): string {
  return str.replace(/\s+/g, "-");
}

export function generateFrontmatter(
  metadata: Record<string, string | number>
): string {
  return dedent(`
      ---
      ${Object.entries(metadata)
        .map(
          ([key, value]) =>
            `${key}: "${typeof value === "string" ? value.trim() : value}"`
        )
        .join("\n")}
      ---
    `);
}

export async function ensureDirectoryExists(dirPath: string): Promise<void> {
  try {
    await fs.access(dirPath);
  } catch {
    await mkdir(dirPath, { recursive: true });
  }
}

export async function createFileIfNotExists(
  filePath: string,
  content: string
): Promise<void> {
  try {
    await fs.access(filePath);
  } catch {
    await writeFile(filePath, content);
  }
}
