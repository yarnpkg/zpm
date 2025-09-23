import { useSuspenseQuery } from "@tanstack/react-query";
import DOMPurify from "dompurify";
import gitUrlParse from "git-url-parse";
import type {
  PackageInfo,
  ReleaseFile,
  ReleaseJsDelivrInfo,
  ReleaseNpmInfo,
} from "src/types/package";
import { normalizeRepoUrl, resolveQualifier } from "@/utils/helpers";
import { resolve as resolveExports } from "resolve.exports";
import { marked, type Tokens } from "marked";

export function usePackageInfo(name: string) {
  return useSuspenseQuery({
    queryKey: [`packageRegistryMetadata`, name],
    queryFn: async (): Promise<PackageInfo> => {
      const req = await fetch(`https://registry.yarnpkg.com/${name}`);
      const res = await req.json();

      return res;
    },
  }).data!;
}

export function usePackageExists(name: string) {
  return useSuspenseQuery({
    queryKey: [`packageExists`, name],
    queryFn: async () => {
      if (name === null) return false;

      const req = await fetch(
        `https://cdn.jsdelivr.net/npm/${name}/package.json`
      );

      return req.status === 200;
    },
  });
}

export function useReleaseInfo({
  name,
  version,
}: {
  name?: string;
  version?: string;
}): ReleaseJsDelivrInfo {
  return useSuspenseQuery({
    queryKey: [`packageFiles`, name, version],
    queryFn: async () => {
      const req = await fetch(
        `https://data.jsdelivr.com/v1/package/npm/${name}@${version}/flat`
      );
      const res = await req.json();

      const fileSet = new Set<string>();
      for (const file of res.files) fileSet.add(file.name);

      return { files: res.files as Array<ReleaseFile>, fileSet };
    },
  }).data!;
}

export function useReleaseFile({
  name,
  version,
  path,
}: {
  name: string;
  version: string;
  path?: string;
}) {
  return useSuspenseQuery({
    queryKey: [`packageFile`, name, version, path],
    queryFn: async () => {
      if (path === null) return null;

      const req = await fetch(
        `https://cdn.jsdelivr.net/npm/${name}@${version}${path}`
      );
      const res = await req.text();

      return res;
    },
  }).data!;
}

export function useReleaseReadme({
  name,
  readme,
  version,
  versions,
}: {
  name: string;
  readme?: string;
  version: string;
  versions: any;
}) {
  const releaseInfo = useReleaseInfo({ name, version });

  const readmeFile =
    versions?.[version]?.npm?.readme ??
    releaseInfo.files.find((entry: ReleaseFile) => {
      return entry.name.toLowerCase() === `/readme.md`;
    });

  const readmeContent = useReleaseFile({
    name,
    version,
    path: readmeFile?.name ?? null,
  });

  const readmeText =
    readmeContent ?? versions?.[version]?.npm?.readme ?? readme;

  const domPurify = DOMPurify();

  // Fix relative URLs for images
  domPurify.addHook("uponSanitizeAttribute", (node, data) => {
    if (
      data.attrName === "src" &&
      !data.attrValue.startsWith("//") &&
      !data.attrValue.includes(":")
    ) {
      const url = new URL(data.attrValue, "https://example.org").pathname;
      if (releaseInfo.files.some((entry) => entry.name === url)) {
        data.attrValue = `https://cdn.jsdelivr.net/npm/${name}@${version}${url}`;
      } else if (versions?.[version].repository?.url) {
        const normalizedRepositoryUrl = normalizeRepoUrl(
          versions?.[version].repository?.url
        );
        const repoInfo = gitUrlParse(normalizedRepositoryUrl);

        if (
          repoInfo.owner &&
          repoInfo.name &&
          repoInfo.source === "github.com"
        ) {
          data.attrValue = `https://cdn.jsdelivr.net/gh/${repoInfo.owner}/${repoInfo.name}${url}`;
        }
      }
    }
  });

  // Escape for code blocks
  function escapeHtml(str: string) {
    return str
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#039;");
  }

  marked.use({
    renderer: {
      code({ text }: Tokens.Code) {
        return `
          <figure
            class="not-content border border-white/5 rounded-2xl bg-white/3"
          >
            <div class="flex items-center gap-2 border-b border-white/5 p-4">
              <span class="size-3 rounded-full bg-[#DB2A4D] border border-[#DB2A4D]"></span>
              <span class="size-3 rounded-full bg-[#FFB888] border border-[#FFB888]"></span>
              <span class="size-3 rounded-full bg-[#B2FFB5] border border-[#B2FFB5]"></span>
            </div>
            <div class="p-4 overflow-x-auto font-montserrat">${escapeHtml(
              text
            )}</div>
          </figure>
        `.trim();
      },
    },
    breaks: true,
  });

  const rawHtml = marked.parse(readmeText ?? "");

  const safeHtml = domPurify.sanitize(rawHtml);

  return safeHtml;
}

export function useResolution(
  {
    name,
    version,
    versionData,
  }: { name: string; version: string; versionData?: any },
  {
    mainFields,
    conditions,
  }: {
    mainFields: Array<keyof ReleaseNpmInfo>;
    conditions: Array<string>;
  }
) {
  const releaseData = useReleaseInfo({
    name,
    version,
  });

  const releaseInfo = {
    name,
    version,
    npm: versionData,
    jsdelivr: releaseData,
  };

  let exportsResolution;

  try {
    exportsResolution = resolveExports(versionData?.npm, `.`, {
      conditions,
    })?.[0];
  } catch {}

  if (versionData?.npm?.exports && !exportsResolution) return null;

  if (exportsResolution)
    return resolveQualifier(releaseInfo, exportsResolution);

  for (const mainField of mainFields) {
    const resolution = resolveQualifier(
      releaseInfo,
      versionData?.npm?.[mainField] || `.`
    );
    if (resolution !== null) {
      return resolution;
    }
  }

  return null;
}

export function useWeeklyDownloads(name: string) {
  return useSuspenseQuery({
    queryKey: [`weeklyDownloads`, name],
    queryFn: async () => {
      const req = await fetch(
        `https://api.npmjs.org/downloads/range/last-week/${name}`
      );
      const res = await req.json();
      return res;
    },
  });
}
