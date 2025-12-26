import {useSuspenseQuery} from '@tanstack/react-query';

interface YarnVersion {
  label: string;
  href: string;
}

interface NpmResponse {
  name: string;
  version: string;
  "dist-tags": Record<string, string>;
  versions: Record<string, {
    version: string;
    dist: {
      tarball: string;
    };
  }>;
}

interface RepoResponse {
  tags: Array<string>;
  latest: Record<string, string>;
}

interface RegistryMetadata {
  npm: NpmResponse;
  repo: RepoResponse;
}

function useRegistryMetadata() {
  return useSuspenseQuery({
    queryKey: [`registryMetadata`],
    queryFn: async (): Promise<RegistryMetadata> => {
      const repoRequest = fetch(`https://repo.yarnpkg.com/tags`);
      const npmRequest = fetch(`https://registry.npmjs.org/yarn`, {
        headers: {accept: `application/vnd.npm.install-v1+json`},
      });

      const [npmResponse, repoResponse] = await Promise.all([
        npmRequest.then(res => res.json()),
        repoRequest.then(res => res.json()),
      ]);

      return {
        npm: npmResponse,
        repo: repoResponse,
      };
    },
  }).data!;
}

export function useYarnVersions(): [YarnVersion, YarnVersion, YarnVersion] {
  const {npm, repo} = useRegistryMetadata();

  const foundV3 = repo.tags.find((v: string) => v.startsWith(`v3`));

  return [
    {
      label: `Master (4.9.1-dev)`,
      href: `https://yarnpkg.com/getting-started`,
    },
    {
      label: foundV3 || `3.8.7`,
      href: `https://v3.yarnpkg.com`,
    },
    {
      label: npm[`dist-tags`].latest || `1.22.22`,
      href: `https://classic.yarnpkg.com/en/docs`,
    },
  ];
}

export function useYarnReleaseVersions(): Record<string, string> {
  const {repo} = useRegistryMetadata();
  return repo.latest;
}
