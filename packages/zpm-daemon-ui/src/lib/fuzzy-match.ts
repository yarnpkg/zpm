export interface FuzzyMatch {
  score: number;
  ranges: Array<[number, number]>;
}

function isSeparator(ch: string): boolean {
  return ch === `-` || ch === `.` || ch === `/` || ch === `:` || ch === `_`;
}

function isWordBoundary(target: string, i: number): boolean {
  if (i === 0) return true;
  const prev = target.charAt(i - 1);
  const curr = target.charAt(i);
  if (isSeparator(prev)) return true;
  if (prev >= `a` && prev <= `z` && curr >= `A` && curr <= `Z`) return true;
  return false;
}

/**
 * VSCode-style fuzzy match. Characters in `pattern` must appear in order in
 * `target` but not necessarily consecutively. Scoring favours consecutive
 * runs, word-boundary matches, and matches near the start.
 *
 * Returns `null` when there is no match.
 */
export function fuzzyMatch(pattern: string, target: string): FuzzyMatch | null {
  if (pattern.length === 0)
    return {score: 0, ranges: []};

  const pLower = pattern.toLowerCase();
  const tLower = target.toLowerCase();

  // Quick reject: every pattern char must exist in order.
  {
    let j = 0;
    for (let i = 0; i < pLower.length; i++) {
      j = tLower.indexOf(pLower.charAt(i), j);
      if (j === -1) return null;
      j++;
    }
  }

  // Recursive best-match with memoisation.
  // State: (pi, ti, wasConsecutive) → best score, with path tracking.
  const pLen = pLower.length;
  const tLen = tLower.length;

  const memo = new Map<number, number>();
  const pathMemo = new Map<number, number[]>();

  function key(pi: number, ti: number, consec: boolean): number {
    return (pi * tLen + ti) * 2 + (consec ? 1 : 0);
  }

  function solve(pi: number, ti: number, consec: boolean): number {
    if (pi === pLen) return 0;
    if (ti === tLen) return -Infinity;
    if (tLen - ti < pLen - pi) return -Infinity; // Not enough chars left.

    const k = key(pi, ti, consec);
    const cached = memo.get(k);
    if (cached !== undefined) return cached;

    let best = -Infinity;
    let bestPath: number[] = [];

    const ch = pLower.charAt(pi);

    for (let t = ti; t <= tLen - (pLen - pi); t++) {
      if (tLower.charAt(t) !== ch) continue;

      let bonus = 0;

      // Word boundary bonus.
      if (isWordBoundary(target, t)) bonus += 6;

      // Exact case bonus.
      if (pattern.charAt(pi) === target.charAt(t)) bonus += 1;

      // Consecutive bonus: if previous char was matched at t-1.
      const isConsec = t === ti && consec;
      if (isConsec) bonus += 8;

      // Penalty for distance from start.
      bonus -= t * 0.1;

      const rest = solve(pi + 1, t + 1, true);
      if (rest === -Infinity) continue;

      // Also try skipping (not being consecutive for the next char).
      const skipRest = solve(pi + 1, t + 1, false);

      let total: number;
      let sub: number[];
      if (rest >= skipRest) {
        total = bonus + rest;
        sub = pathMemo.get(key(pi + 1, t + 1, true)) ?? [];
      } else {
        total = bonus + skipRest;
        sub = pathMemo.get(key(pi + 1, t + 1, false)) ?? [];
      }

      if (total > best) {
        best = total;
        bestPath = [t, ...sub];
      }
    }

    memo.set(k, best);
    pathMemo.set(k, bestPath);
    return best;
  }

  const score = solve(0, 0, false);
  if (score === -Infinity) return null;

  const indices = pathMemo.get(key(0, 0, false)) ?? [];
  if (indices.length === 0) return null;

  // Merge consecutive indices into ranges.
  const ranges: Array<[number, number]> = [];
  let start = indices[0]!;
  let end = start + 1;

  for (let i = 1; i < indices.length; i++) {
    if (indices[i] === end) {
      end++;
    } else {
      ranges.push([start, end]);
      start = indices[i]!;
      end = start + 1;
    }
  }
  ranges.push([start, end]);

  return {score, ranges};
}
