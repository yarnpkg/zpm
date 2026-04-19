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

export function useVersion(releaseLine: string, channel: string) {
  return useSuspenseQuery({
    queryKey: [`versions`, releaseLine, channel],
    queryFn: async (): Promise<string> => {
      const res = await fetch(`https://repo.yarnpkg.com/channels/${releaseLine}/${channel}`);
      const data = await res.text();

      return data.trim();
    },
  }).data!;
}
