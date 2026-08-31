type Point = [number, number];

export interface Constellation {
  name: string;
  stars: Array<Point>;
  edges: Array<[number, number]>;
}

function compile(name: string, strokes: Array<Array<Point>>): Constellation {
  const stars: Array<Point> = [];
  const edges: Array<[number, number]> = [];
  const EPS = 0.02;

  const findOrAdd = ([x, y]: Point): number => {
    for (let i = 0; i < stars.length; i++) {
      const [sx, sy] = stars[i];
      if (Math.abs(sx - x) < EPS && Math.abs(sy - y) < EPS) {
        return i;
      }
    }
    stars.push([x, y]);
    return stars.length - 1;
  };

  for (const stroke of strokes) {
    const idxs = stroke.map(findOrAdd);
    for (let i = 1; i < idxs.length; i++) {
      if (idxs[i - 1] !== idxs[i]) {
        edges.push([idxs[i - 1], idxs[i]]);
      }
    }
  }

  return {name, stars, edges};
}

const L = 0.15, R = 0.85, T = 0.10, B = 0.90, M = 0.50, MY = 0.50;

const runes: Array<Constellation> = [
  compile(`fehu`, [[[L, T], [L, B]], [[L, 0.25], [R, 0.10]], [[L, 0.45], [R, 0.30]]]),
  compile(`uruz`, [[[L, B], [L, T], [R, 0.30], [R, B]]]),
  compile(`thurisaz`, [[[L, T], [L, B]], [[L, 0.30], [R, MY], [L, 0.70]]]),
  compile(`ansuz`, [[[L, T], [L, B]], [[L, 0.22], [R, 0.08]], [[L, 0.40], [0.60, 0.30]]]),
  compile(`raidho`, [[[L, T], [L, B]], [[L, T], [R, 0.30], [L, MY]], [[L, MY], [R, B]]]),
  compile(`kenaz`, [[[L, T], [R, MY], [L, B]]]),
  compile(`gebo`, [[[L, T], [R, B]], [[R, T], [L, B]]]),
  compile(`wunjo`, [[[L, T], [L, B]], [[L, T], [R, 0.25], [L, MY]]]),
  compile(`hagalaz`, [[[L, T], [L, B]], [[R, T], [R, B]], [[L, MY], [R, MY]]]),
  compile(`nauthiz`, [[[L, T], [L, B]], [[L, 0.30], [R, 0.70]]]),
  compile(`isa`, [[[M, T], [M, B]]]),
  compile(`jera`, [[[L, 0.30], [0.40, T], [M, 0.30]], [[M, 0.70], [0.60, B], [R, 0.70]]]),
  compile(`eihwaz`, [[[L, 0.25], [M, T]], [[M, T], [M, B]], [[M, B], [R, 0.75]]]),
  compile(`perthro`, [[[R, T], [L, T], [L, B], [R, B]]]),
  compile(`algiz`, [[[M, B], [M, MY]], [[M, MY], [L, T]], [[M, MY], [R, T]]]),
  compile(`sowilo`, [[[R, T], [0.40, 0.30], [0.60, 0.70], [L, B]]]),
  compile(`tiwaz`, [[[M, T], [M, B]], [[L, 0.30], [M, T], [R, 0.30]]]),
  compile(`berkano`, [[[L, T], [L, B]], [[L, T], [R, 0.28], [L, MY]], [[L, MY], [R, 0.72], [L, B]]]),
  compile(`ehwaz`, [[[L, B], [L, T]], [[L, T], [R, B]], [[R, B], [R, T]], [[L, 0.30], [R, 0.30]]]),
  compile(`mannaz`, [[[L, B], [L, T]], [[R, B], [R, T]], [[L, T], [R, B]], [[R, T], [L, B]]]),
  compile(`laguz`, [[[L, T], [L, B]], [[L, T], [0.55, 0.25]]]),
  compile(`ingwaz`, [[[M, T], [R, MY], [M, B], [L, MY], [M, T]]]),
  compile(`dagaz`, [[[L, T], [L, B]], [[R, T], [R, B]], [[L, T], [R, B]], [[L, B], [R, T]]]),
  compile(`othala`, [[[M, T], [R, 0.35], [0.65, 0.65], [M, MY], [0.35, 0.65], [L, 0.35], [M, T]], [[0.35, 0.65], [0.25, B]], [[0.65, 0.65], [0.75, B]]]),
];

const extensions: Array<Constellation> = [
  compile(`ear`, [[[M, T], [M, B]], [[L, 0.25], [M, T], [R, 0.25]], [[L, 0.50], [M, 0.30]]]),
  compile(`cweorth`, [[[M, B], [L, T]], [[M, B], [M, T]], [[M, B], [R, T]]]),
  compile(`calc`, [[[M, T], [M, MY]], [[M, MY], [L, B]], [[M, MY], [R, B]]]),
  compile(`stan`, [[[L, B], [L, 0.30], [M, T], [R, 0.30], [R, B]], [[L, 0.30], [R, 0.30]]]),
  compile(`gar`, [[[M, T], [M, B]], [[L, MY], [R, MY]], [[L, T], [R, B]], [[R, T], [L, B]]]),
  compile(`yr`, [[[M, T], [M, B]], [[L, B], [M, MY], [R, B]]]),
  compile(`hagall`, [[[M, T], [M, B]], [[L, 0.25], [R, 0.75]], [[R, 0.25], [L, 0.75]]]),
  compile(`bind-gebo-isa`, [[[L, T], [R, B]], [[R, T], [L, B]], [[M, T], [M, B]]]),
  compile(`bind-tiwaz-stack`, [[[M, T], [M, B]], [[0.30, 0.20], [M, 0.10], [0.70, 0.20]], [[0.30, 0.60], [M, MY], [0.70, 0.60]]]),
  compile(`bind-algiz-isa`, [[[M, T], [M, B]], [[L, T], [M, 0.35], [R, T]]]),
  compile(`bind-fehu-2`, [[[L, T], [L, B]], [[L, 0.22], [M, 0.10]], [[L, 0.40], [0.55, 0.30]], [[R, T], [R, B]], [[R, 0.22], [0.70, 0.10]]]),
  compile(`bind-othala`, [[[M, T], [R, MY], [M, B], [L, MY], [M, T]], [[L, MY], [0.05, 0.80]], [[R, MY], [0.95, 0.80]]]),
  compile(`bind-ehwaz-rot`, [[[L, T], [L, B]], [[R, T], [R, B]], [[L, T], [R, B]], [[L, 0.30], [R, 0.30]], [[L, 0.70], [R, 0.70]]]),
  compile(`aegishjalmr`, [[[M, T], [M, B]], [[L, MY], [R, MY]], [[0.25, 0.25], [0.75, 0.75]], [[0.75, 0.25], [0.25, 0.75]]]),
  compile(`valknut`, [[[M, T], [L, B], [R, B], [M, T]], [[0.35, 0.40], [M, B]]]),
  compile(`vegvisir`, [[[M, T], [M, B]], [[L, MY], [R, MY]], [[M, 0.20], [0.40, 0.10]], [[M, 0.20], [0.60, 0.10]], [[M, 0.80], [0.40, 0.90]], [[M, 0.80], [0.60, 0.90]]]),
];

let seed = 0xbeefcafe;
const rnd = (): number => {
  seed = (seed * 1664525 + 1013904223) | 0; return ((seed >>> 0) % 1e6) / 1e6;
};
const pick = <T>(arr: Array<T>): T => arr[Math.floor(rnd() * arr.length)];

const makeStroke = (kind: string): Array<Point> => {
  switch (kind) {
    case `stave`: return [[M, T], [M, B]];
    case `hstave`: return [[L, MY], [R, MY]];
    case `slash-dr`: return [[L, T], [R, B]];
    case `slash-ur`: return [[L, B], [R, T]];
    case `chevron-r`: return [[L, T], [R, MY], [L, B]];
    case `chevron-l`: return [[R, T], [L, MY], [R, B]];
    case `chevron-u`: return [[L, B], [M, T], [R, B]];
    case `chevron-d`: return [[L, T], [M, B], [R, T]];
    case `top-arm-r`: return [[M, T], [0.70 + rnd() * 0.15, 0.25 + rnd() * 0.1]];
    case `top-arm-l`: return [[M, T], [0.30 - rnd() * 0.15, 0.25 + rnd() * 0.1]];
    case `bot-arm-r`: return [[M, B], [0.70 + rnd() * 0.15, 0.75 - rnd() * 0.1]];
    case `bot-arm-l`: return [[M, B], [0.30 - rnd() * 0.15, 0.75 - rnd() * 0.1]];
    case `side-arm`: { const y = 0.30 + rnd() * 0.4; return [[L, y], [M, 0.25 + rnd() * 0.5]]; }
    case `diamond-s`: return [[M, 0.30], [0.62, MY], [M, 0.70], [0.38, MY], [M, 0.30]];
    case `triangle-r`: return [[L, 0.30], [R, MY], [L, 0.70]];
    case `triangle-l`: return [[R, 0.30], [L, MY], [R, 0.70]];
    case `half-x`: return [[L, 0.30], [R, 0.70]];
    case `half-x2`: return [[L, 0.70], [R, 0.30]];
  }
  return [[M, T], [M, B]];
};

const strokeKinds = [`stave`, `hstave`, `slash-dr`, `slash-ur`, `chevron-r`, `chevron-l`, `chevron-u`, `chevron-d`, `top-arm-r`, `top-arm-l`, `bot-arm-r`, `bot-arm-l`, `side-arm`, `diamond-s`, `triangle-r`, `triangle-l`, `half-x`, `half-x2`];

const combined = [...runes, ...extensions];
const needed = 80 - combined.length;

for (let i = 0; i < needed; i++) {
  const count = 2 + Math.floor(rnd() * 2);
  const strokes: Array<Array<Point>> = [];
  const used = new Set<string>();
  for (let j = 0; j < count; j++) {
    let k = pick(strokeKinds);
    let guard = 0;
    while (used.has(k) && guard++ < 4) k = pick(strokeKinds);
    used.add(k);
    strokes.push(makeStroke(k));
  }
  if (rnd() < 0.55 && !used.has(`stave`)) strokes.unshift(makeStroke(`stave`));
  combined.push(compile(`bindrune-${i}`, strokes));
}

export const CONSTELLATION_LIBRARY: Array<Constellation> = combined.slice(0, 80);
