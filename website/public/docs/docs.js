/* ─────────────── Shared starfield + theme (minimal) ─────────────── */
(function () {
  // Theme
  const saved = localStorage.getItem(`yarn-theme`) || `dark`;
  document.documentElement.setAttribute(`data-theme`, saved);
  window.__theme = saved;

  function setTheme(t) {
    document.documentElement.setAttribute(`data-theme`, t);
    localStorage.setItem(`yarn-theme`, t);
    window.__theme = t;
    window.dispatchEvent(new CustomEvent(`themechange`, {detail: t}));
  }
  window.__setTheme = setTheme;

  const btn = document.getElementById(`theme-toggle`);
  if (btn) btn.addEventListener(`click`, () => setTheme(window.__theme === `dark` ? `light` : `dark`));

  // Starfield canvas (lighter, non-interactive)
  const canvas = document.getElementById(`stars`);
  if (!canvas) return;
  const ctx = canvas.getContext(`2d`);
  let W = window.innerWidth, H = window.innerHeight, DPR = 1;
  let stars = [];
  function resize() {
    DPR = Math.min(window.devicePixelRatio || 1, 2);
    W = window.innerWidth;
    H = window.innerHeight;
    canvas.width = W * DPR;
    canvas.height = H * DPR;
    canvas.style.width = `${W}px`;
    canvas.style.height = `${H}px`;
    ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    init();
  }
  function init() {
    // Fewer stars than the landing — reading comfort
    const count = Math.round(180 * (W * H) / (1920 * 1080));
    stars = [];
    for (let i = 0; i < count; i++) {
      stars.push({
        x: Math.random() * W,
        y: Math.random() * H,
        r: Math.random() * 1.1 + 0.2,
        a: Math.random() * 0.5 + 0.25,
        tp: Math.random() * Math.PI * 2,
        ts: Math.random() * 0.6 + 0.2,
      });
    }
  }
  let t = 0;
  function tick(ts) {
    t = ts * 0.001;
    ctx.clearRect(0, 0, W, H);
    const isDark = window.__theme === `dark`;
    const color = isDark ? `255,255,255` : `255,200,100`;
    for (const s of stars) {
      const tw = 0.55 + 0.45 * Math.sin(t * s.ts + s.tp);
      const a = s.a * tw * 0.7;
      ctx.beginPath();
      ctx.arc(s.x, s.y, s.r, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(${color},${a.toFixed(3)})`;
      ctx.fill();
    }
    requestAnimationFrame(tick);
  }
  window.addEventListener(`resize`, resize);
  resize();
  requestAnimationFrame(tick);
})();

/* ─────────────── Docs-specific features ─────────────── */
(function () {
  /* Toast */
  const toast = document.createElement(`div`);
  toast.className = `toast`;
  toast.setAttribute(`role`, `status`);
  toast.setAttribute(`aria-live`, `polite`);
  document.body.appendChild(toast);
  let toastTimer;
  function showToast(msg) {
    toast.textContent = msg;
    toast.classList.add(`show`);
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toast.classList.remove(`show`), 1800);
  }
  window.__showToast = showToast;

  /* Heading + field anchors: click = copy URL (anchors injected at build time) */
  document.addEventListener(`click`, e => {
    const anchor = e.target.closest(`.heading-anchor, .field-anchor`);
    if (!anchor) return;
    e.preventDefault();
    const href = anchor.getAttribute(`href`);
    history.replaceState(null, ``, href);
    const url = location.origin + location.pathname + href;
    navigator.clipboard?.writeText(url).then(
      () => showToast(`Link copied`),
      () => showToast(`Press ⌘C to copy`),
    );
  });

  /* Copy buttons on terminal + code blocks */
  document.querySelectorAll(`.terminal, .code-block`).forEach(el => {
    if (el.querySelector(`.copy-btn`)) return;
    const btn = document.createElement(`button`);
    btn.className = `copy-btn`;
    btn.setAttribute(`aria-label`, `Copy code`);
    btn.innerHTML = `<svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="3" y="3" width="7" height="7" rx="1"/><path d="M2 8V2h6" opacity="0.6"/></svg>`;
    btn.addEventListener(`click`, () => {
      const toCopy = el.classList.contains(`terminal`)
        ? Array.from(el.querySelectorAll(`.term-line`))
          .filter(l => !l.classList.contains(`out`) && !l.classList.contains(`comment`))
          .map(l => l.textContent)
          .join(`\n`)
        : (el.querySelector(`pre code`) || el.querySelector(`pre`)).textContent;
      navigator.clipboard?.writeText(toCopy).then(() => {
        btn.classList.add(`copied`);
        btn.innerHTML = `<svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M2 6l3 3 5-6"/></svg>`;
        setTimeout(() => {
          btn.classList.remove(`copied`);
          btn.innerHTML = `<svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="3" y="3" width="7" height="7" rx="1"/><path d="M2 8V2h6" opacity="0.6"/></svg>`;
        }, 1400);
      });
    });
    const target = el.classList.contains(`code-block`) ? el.querySelector(`pre`) || el : el;
    target.appendChild(btn);
  });

  /* Scrollspy for sidebar: mark active link based on scroll */
  const sbLinks = document.querySelectorAll(`.docs-sidebar a.sb-link[data-section]`);
  if (sbLinks.length) {
    const sections = Array.from(document.querySelectorAll(`.prose h2[id], .prose h3[id]`));
    function onScroll() {
      const y = window.scrollY + 120;
      let activeId = sections[0]?.id;
      for (const s of sections)  if (s.offsetTop <= y) activeId = s.id;
      sbLinks.forEach(a => {
        const want = a.getAttribute(`href`)?.replace(/^#/, ``);
        a.classList.toggle(`active`, want === activeId);
      });
    }
    window.addEventListener(`scroll`, onScroll, {passive: true});
    onScroll();
  }
})();
