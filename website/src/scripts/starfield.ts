import {CONSTELLATION_LIBRARY} from '../data/constellations';

export interface StarfieldState {
  theme: string;
  starDensity: number;
  starSpeed: number;
  starOpacity: number;
  constellations: boolean;
}

interface Star {
  theta: number;
  phi: number;
  r: number;
  baseAlpha: number;
  twinklePhase: number;
  twinkleSpeed: number;
  isConstellation?: boolean;
  _visible?: boolean;
  _sx?: number;
  _sy?: number;
  _depth?: number;
}

interface ShootingStar {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number;
  maxLife: number;
  trail: Array<{x: number, y: number}>;
}

export function createStarfield(
  canvas: HTMLCanvasElement,
  state: StarfieldState,
): {initStars: () => void} {
  const ctx = canvas.getContext(`2d`)!;
  let stars: Array<Star> = [];
  let constellationStars: Array<Star> = [];
  let constellations: Array<[Star, Star]> = [];
  let shootingStars: Array<ShootingStar> = [];
  let W = window.innerWidth, H = window.innerHeight, DPR = 1;

  let catImg: HTMLImageElement | null = null;
  let catImgReady = false;
  let catRect: {x: number, y: number, w: number, h: number} | null = null;

  (function loadCat() {
    const im = new Image();
    im.crossOrigin = `anonymous`;
    im.onload = () => {
      catImg = im; catImgReady = true; computeCatPath();
    };
    im.src = `cat.png`;
  })();

  function computeCatPath() {
    const el = document.getElementById(`cat-img`);
    if (!el) {
      catRect = null; return;
    }
    const rect = el.getBoundingClientRect();
    if (catImg) {
      const iw = catImg.naturalWidth, ih = catImg.naturalHeight;
      const scale = Math.min(rect.width / iw, rect.height / ih);
      const dw = iw * scale, dh = ih * scale;
      const dx = rect.left + (rect.width - dw) / 2;
      const dy = rect.top + (rect.height - dh) / 2;
      catRect = {x: dx, y: dy, w: dw, h: dh};
    } else {
      catRect = {x: rect.left, y: rect.top, w: rect.width, h: rect.height};
    }
  }

  function resize() {
    DPR = Math.min(window.devicePixelRatio || 1, 2);
    W = window.innerWidth;
    H = window.innerHeight;
    canvas.width = W * DPR;
    canvas.height = H * DPR;
    canvas.style.width = `${W}px`;
    canvas.style.height = `${H}px`;
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    computeCatPath();
    initStars();
  }

  function initStars() {
    const density = state.starDensity;
    const targetOnScreen = (density / 100) * 2700;
    const base = targetOnScreen * 3.0;
    const count = Math.max(0, Math.round(base * (W * H) / (1920 * 1080)));

    stars = [];
    for (let i = 0; i < count; i++) {
      const u = Math.random(), v = Math.random();
      const theta = 2 * Math.PI * u;
      const phi = Math.acos(2 * v - 1);
      stars.push({theta, phi, r: Math.random() * 1.2 + 0.25, baseAlpha: Math.random() * 0.6 + 0.3, twinklePhase: Math.random() * Math.PI * 2, twinkleSpeed: Math.random() * 0.8 + 0.3});
    }

    constellations = [];
    constellationStars = [];
    const MIN_ANGLE = 0.42;
    const MIN_COS = Math.cos(MIN_ANGLE);
    const anchors: Array<{theta: number, phi: number}> = [];
    const MAX_ATTEMPTS = 4000;
    let attempts = 0;
    while (anchors.length < 40 && attempts++ < MAX_ATTEMPTS) {
      const u = Math.random(), v = Math.random();
      const theta0 = 2 * Math.PI * u;
      const phi0 = Math.acos(2 * v - 1);
      const ok = anchors.every(a => {
        const dot = Math.sin(phi0) * Math.sin(a.phi) * Math.cos(theta0 - a.theta) + Math.cos(phi0) * Math.cos(a.phi);
        return dot < MIN_COS;
      });
      if (ok) {
        anchors.push({theta: theta0, phi: phi0});
      }
    }

    const shuffled = CONSTELLATION_LIBRARY.slice().sort(() => Math.random() - 0.5);

    for (let idx = 0; idx < anchors.length && idx < shuffled.length; idx++) {
      const anchor = anchors[idx];
      const pattern = shuffled[idx];
      const sP = Math.sin(anchor.phi), cP = Math.cos(anchor.phi);
      const sT = Math.sin(anchor.theta), cT = Math.cos(anchor.theta);
      const ex = {x: -sT, y: 0, z: cT};
      const ey = {x: cP * cT, y: -sP, z: cP * sT};
      const n = {x: sP * cT, y: cP, z: sP * sT};
      const sizeRad = 0.14 + Math.random() * 0.08;
      const rot = Math.random() * Math.PI * 2;
      const nodes = pattern.stars.map(([nx, ny]) => {
        const lx = (nx - 0.5) * 2 * sizeRad + (Math.random() - 0.5) * 0.008;
        const ly = (ny - 0.5) * 2 * sizeRad + (Math.random() - 0.5) * 0.008;
        const rx = lx * Math.cos(rot) - ly * Math.sin(rot);
        const ry = lx * Math.sin(rot) + ly * Math.cos(rot);
        const px = n.x + rx * ex.x + ry * ey.x;
        const py = n.y + rx * ex.y + ry * ey.y;
        const pz = n.z + rx * ex.z + ry * ey.z;
        const mag = Math.hypot(px, py, pz);
        const ux = px / mag, uy = py / mag, uz = pz / mag;
        const phi = Math.acos(Math.max(-1, Math.min(1, uy)));
        const theta = Math.atan2(uz, ux);
        const star: Star = {theta, phi, r: 1.4 + Math.random() * 0.6, baseAlpha: 0.9 + Math.random() * 0.1, twinklePhase: Math.random() * Math.PI * 2, twinkleSpeed: 0.6 + Math.random() * 0.5, isConstellation: true};
        stars.push(star);
        constellationStars.push(star);
        return star;
      });
      for (const [a, b] of pattern.edges) {
        constellations.push([nodes[a], nodes[b]]);
      }
    }
  }

  function spawnShootingStar() {
    const startX = Math.random() * W * 0.6;
    const startY = Math.random() * 300 + 40;
    const angle = Math.PI / 4 + (Math.random() - 0.5) * 0.3;
    const speed = 8 + Math.random() * 4;
    shootingStars.push({x: startX, y: startY, vx: Math.cos(angle) * speed, vy: Math.sin(angle) * speed, life: 0, maxLife: 60 + Math.random() * 30, trail: []});
  }

  let t = 0;
  let lastShoot = 0;
  let lastFrameTs = 0;
  let rotationAngle = 0;
  const ROT_AXIS = (() => {
    const v = {x: 0.25, y: 0.92, z: 0.30}; const m = Math.hypot(v.x, v.y, v.z); return {x: v.x / m, y: v.y / m, z: v.z / m};
  })();

  function rotateAroundAxis(x: number, y: number, z: number, axis: {x: number, y: number, z: number}, ang: number) {
    const c = Math.cos(ang), s = Math.sin(ang);
    const {x: ux, y: uy, z: uz} = axis;
    const dot = ux * x + uy * y + uz * z;
    return {x: x * c + (uy * z - uz * y) * s + ux * dot * (1 - c), y: y * c + (uz * x - ux * z) * s + uy * dot * (1 - c), z: z * c + (ux * y - uy * x) * s + uz * dot * (1 - c)};
  }

  function tick(ts: number) {
    const dt = lastFrameTs ? Math.min(0.05, (ts - lastFrameTs) / 1000) : 0;
    lastFrameTs = ts;
    t = ts * 0.001;
    rotationAngle += (state.starSpeed || 0) * dt;
    ctx.clearRect(0, 0, W, H);
    const isDark = state.theme === `dark`;
    const starColor = isDark ? `255, 255, 255` : `255, 200, 100`;
    const conColor = isDark ? `200, 210, 255` : `12, 16, 48`;
    ctx.save();
    const opacityMul = Math.max(0, Math.min(1, (state.starOpacity ?? 100) / 100));
    const cx = W / 2, cy = H / 2;
    const projScale = Math.hypot(W, H) * 0.55;
    for (const s of stars) {
      const sinPhi = Math.sin(s.phi);
      const bx = sinPhi * Math.cos(s.theta);
      const by = Math.cos(s.phi);
      const bz = sinPhi * Math.sin(s.theta);
      const p = rotateAroundAxis(bx, by, bz, ROT_AXIS, rotationAngle);
      s._visible = (p.z >= -0.1);
      s._sx = cx + p.x * projScale;
      s._sy = cy + p.y * projScale;
      s._depth = p.z;
      if (!s._visible) continue;
      if (s._sx < -40 || s._sx > W + 40 || s._sy < -40 || s._sy > H + 40) continue;
      const depthFade = Math.min(1, Math.max(0, (p.z + 0.1) / 0.4));
      const tw = 0.6 + 0.4 * Math.sin(t * s.twinkleSpeed + s.twinklePhase);
      const a = s.baseAlpha * tw * depthFade * opacityMul;
      if (a <= 0.01) continue;
      ctx.beginPath();
      ctx.arc(s._sx, s._sy, s.r, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(${starColor}, ${a.toFixed(3)})`;
      ctx.fill();
      if (s.r > 1.0) {
        ctx.beginPath();
        ctx.arc(s._sx, s._sy, s.r * 3, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${starColor}, ${(a * 0.12).toFixed(3)})`;
        ctx.fill();
      }
    }
    if (state.constellations) {
      ctx.strokeStyle = `rgba(${conColor}, ${isDark ? 0.18 : 0.12})`;
      ctx.lineWidth = 0.6;
      for (const [a, b] of constellations) {
        if (!a._visible || !b._visible) continue;
        const minDepth = Math.min(a._depth!, b._depth!);
        const fade = Math.min(1, Math.max(0, (minDepth + 0.1) / 0.4));
        if (fade <= 0.02) continue;
        ctx.globalAlpha = fade * opacityMul;
        ctx.beginPath();
        ctx.moveTo(a._sx!, a._sy!);
        ctx.lineTo(b._sx!, b._sy!);
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    }
    if (ts - lastShoot > 5000 + Math.random() * 6000 && isDark) {
      spawnShootingStar(); lastShoot = ts;
    }
    shootingStars = shootingStars.filter(ss => {
      ss.x += ss.vx; ss.y += ss.vy; ss.life++;
      ss.trail.push({x: ss.x, y: ss.y});
      if (ss.trail.length > 18) ss.trail.shift();
      const alpha = 1 - (ss.life / ss.maxLife);
      for (let i = 0; i < ss.trail.length; i++) {
        const p = ss.trail[i];
        const ta = (i / ss.trail.length) * alpha;
        ctx.beginPath();
        ctx.arc(p.x, p.y, 1.2 * (i / ss.trail.length), 0, Math.PI * 2);
        ctx.fillStyle = `rgba(255, 240, 200, ${ta.toFixed(3)})`;
        ctx.fill();
      }
      ctx.beginPath();
      ctx.arc(ss.x, ss.y, 1.6, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(255, 250, 230, ${alpha.toFixed(3)})`;
      ctx.fill();
      return ss.life < ss.maxLife;
    });
    if (catImgReady && catRect) {
      ctx.globalCompositeOperation = `destination-out`;
      ctx.drawImage(catImg!, catRect.x, catRect.y, catRect.w, catRect.h);
      ctx.globalCompositeOperation = `source-over`;
    }
    ctx.restore();
    requestAnimationFrame(tick);
  }

  window.addEventListener(`resize`, resize);
  window.addEventListener(`scroll`, () => computeCatPath(), {passive: true});
  window.addEventListener(`load`, () => setTimeout(computeCatPath, 200));
  document.fonts?.ready?.then(() => computeCatPath());

  resize();
  requestAnimationFrame(tick);

  return {initStars};
}
