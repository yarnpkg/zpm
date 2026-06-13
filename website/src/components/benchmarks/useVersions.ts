import {useState, useEffect, useRef} from 'react';

export interface VersionEntry {
  v: string;
  t: number;
}

const VERSION_PACKAGES: Array<[string, string, string | null]> = [
  [`npm`, `npm`, null],
  [`pnpm`, `pnpm`, null],
  [`classic`, `yarn`, `1.`],
  [`yarn`, `@yarnpkg/cli`, null],
];

export function useVersions(benchMinTs: number, benchMaxTs: number, enabled: boolean) {
  const [versions, setVersions] = useState<Record<string, Array<VersionEntry>> | null>(null);
  const [loading, setLoading] = useState(false);
  const fetchedRef = useRef(false);

  useEffect(() => {
    if (!enabled || fetchedRef.current)
      return;

    fetchedRef.current = true;
    setLoading(true);

    Promise.all(
      VERSION_PACKAGES.map(([pm, pkg, prefix]) =>
        fetch(`https://registry.npmjs.org/${pkg}`)
          .then(r => r.ok ? r.json() : null)
          .then(data => {
            if (!data?.time) return [pm, [] as Array<VersionEntry>] as const;
            const entries: Array<VersionEntry> = [];
            let latestBefore: VersionEntry | null = null;

            for (const v in data.time) {
              if (v === `created` || v === `modified`)
                continue;

              if (v.includes(`-`))
                continue;

              if (prefix && !v.startsWith(prefix))
                continue;


              const t = Math.floor(new Date(data.time[v]).getTime() / 1000);
              if (t >= benchMinTs && t <= benchMaxTs) {
                entries.push({v, t});
              } else if (t < benchMinTs) {
                if (!latestBefore || t > latestBefore.t) {
                  latestBefore = {v, t};
                }
              }
            }

            if (latestBefore)
              entries.push(latestBefore);

            entries.sort((a, b) => a.t - b.t);
            return [pm, entries] as const;
          })
          .catch(() => [pm, [] as Array<VersionEntry>] as const),
      ),
    ).then(results => {
      const map: Record<string, Array<VersionEntry>> = {};
      for (const [pm, entries] of results)
        map[pm] = entries;

      setVersions(map);
      setLoading(false);
    });
  }, [enabled, benchMinTs, benchMaxTs]);

  return {versions, loading};
}
