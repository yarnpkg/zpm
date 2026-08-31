(() => {
  const DESIGN_W_DEFAULT = 1920;
  const DESIGN_H_DEFAULT = 1080;
  const OVERLAY_HIDE_MS = 1800;

  const pad2 = n => String(n).padStart(2, `0`);

  const stylesheet = `
    :host {
      position: fixed;
      inset: 0;
      display: block;
      background: transparent;
      color: #fff;
      font-family: -apple-system, BlinkMacSystemFont, "Helvetica Neue", Helvetica, Arial, sans-serif;
      overflow: hidden;
    }

    .stage {
      position: absolute;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
    }

    .canvas {
      position: relative;
      transform-origin: center center;
      flex-shrink: 0;
      background: transparent;
      will-change: transform;
    }

    ::slotted(*) {
      position: absolute !important;
      inset: 0 !important;
      width: 100% !important;
      height: 100% !important;
      box-sizing: border-box !important;
      overflow: hidden;
      opacity: 0;
      pointer-events: none;
      visibility: hidden;
    }
    ::slotted([data-deck-active]) {
      opacity: 1;
      pointer-events: auto;
      visibility: visible;
    }

    .tapzones {
      position: fixed;
      inset: 0;
      display: flex;
      z-index: 2147482000;
      pointer-events: none;
    }
    .tapzone {
      flex: 1;
      pointer-events: auto;
      -webkit-tap-highlight-color: transparent;
    }
    @media (hover: hover) and (pointer: fine) {
      .tapzones { display: none; }
    }

    .overlay {
      position: fixed;
      left: 50%;
      bottom: 22px;
      transform: translate(-50%, 6px) scale(0.92);
      filter: blur(6px);
      display: flex;
      align-items: center;
      gap: 4px;
      padding: 4px;
      background: #000;
      color: #fff;
      border-radius: 999px;
      font-size: 12px;
      font-feature-settings: "tnum" 1;
      letter-spacing: 0.01em;
      opacity: 0;
      pointer-events: none;
      transition: opacity 260ms ease, transform 260ms cubic-bezier(.2,.8,.2,1), filter 260ms ease;
      transform-origin: center bottom;
      z-index: 2147483000;
      user-select: none;
    }
    .overlay[data-visible] {
      opacity: 1;
      pointer-events: auto;
      transform: translate(-50%, 0) scale(1);
      filter: blur(0);
    }

    .btn {
      appearance: none;
      -webkit-appearance: none;
      background: transparent;
      border: 0;
      margin: 0;
      padding: 0;
      color: inherit;
      font: inherit;
      cursor: default;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      height: 28px;
      min-width: 28px;
      border-radius: 999px;
      color: rgba(255,255,255,0.72);
      transition: background 140ms ease, color 140ms ease;
      -webkit-tap-highlight-color: transparent;
    }
    .btn:hover { background: rgba(255,255,255,0.12); color: #fff; }
    .btn:active { background: rgba(255,255,255,0.18); }
    .btn:focus { outline: none; }
    .btn:focus-visible { outline: none; }
    .btn::-moz-focus-inner { border: 0; }
    .btn svg { width: 14px; height: 14px; display: block; }
    .btn.reset,
    .btn.present {
      font-size: 11px;
      font-weight: 500;
      letter-spacing: 0.02em;
      padding: 0 10px 0 12px;
      gap: 6px;
      color: rgba(255,255,255,0.72);
    }
    .btn.reset .kbd,
    .btn.present .kbd {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-width: 16px;
      height: 16px;
      padding: 0 4px;
      font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
      font-size: 10px;
      line-height: 1;
      color: rgba(255,255,255,0.88);
      background: rgba(255,255,255,0.12);
      border-radius: 4px;
    }

    .count {
      font-variant-numeric: tabular-nums;
      color: #fff;
      font-weight: 500;
      padding: 0 8px;
      min-width: 42px;
      text-align: center;
      font-size: 12px;
    }
    .count .sep { color: rgba(255,255,255,0.45); margin: 0 3px; font-weight: 400; }
    .count .total { color: rgba(255,255,255,0.55); }

    .divider {
      width: 1px;
      height: 14px;
      background: rgba(255,255,255,0.18);
      margin: 0 2px;
    }

    @media print {
      :host {
        position: static;
        inset: auto;
        background: none;
        overflow: visible;
        color: inherit;
      }
      .stage { position: static; display: block; }
      .canvas {
        transform: none !important;
        width: auto !important;
        height: auto !important;
        background: none;
        will-change: auto;
      }
      ::slotted(*) {
        position: relative !important;
        inset: auto !important;
        width: var(--deck-design-w) !important;
        height: var(--deck-design-h) !important;
        box-sizing: border-box !important;
        opacity: 1 !important;
        visibility: visible !important;
        pointer-events: auto;
        break-after: page;
        page-break-after: always;
        break-inside: avoid;
        overflow: hidden;
      }
      ::slotted(*:last-child) {
        break-after: auto;
        page-break-after: auto;
      }
      .overlay, .tapzones { display: none !important; }
    }
  `;

  class DeckStage extends HTMLElement {
    static get observedAttributes() {
      return [`width`, `height`, `noscale`];
    }

    constructor() {
      super();
      this._root = this.attachShadow({mode: `open`});
      this._index = 0;
      this._slides = [];
      this._hideTimer = null;
      this._mouseIdleTimer = null;
      this._isPresenting = false;

      this._onKey = this._onKey.bind(this);
      this._onResize = this._onResize.bind(this);
      this._onSlotChange = this._onSlotChange.bind(this);
      this._onMouseMove = this._onMouseMove.bind(this);
      this._onTapBack = this._onTapBack.bind(this);
      this._onTapForward = this._onTapForward.bind(this);
      this._onFullscreenChange = this._onFullscreenChange.bind(this);
      this._togglePresent = this._togglePresent.bind(this);
    }

    get designWidth() {
      return parseInt(this.getAttribute(`width`), 10) || DESIGN_W_DEFAULT;
    }
    get designHeight() {
      return parseInt(this.getAttribute(`height`), 10) || DESIGN_H_DEFAULT;
    }

    connectedCallback() {
      this._render();
      window.addEventListener(`keydown`, this._onKey);
      window.addEventListener(`resize`, this._onResize);
      window.addEventListener(`mousemove`, this._onMouseMove, {passive: true});
      document.addEventListener(`fullscreenchange`, this._onFullscreenChange);
    }

    disconnectedCallback() {
      window.removeEventListener(`keydown`, this._onKey);
      window.removeEventListener(`resize`, this._onResize);
      window.removeEventListener(`mousemove`, this._onMouseMove);
      document.removeEventListener(`fullscreenchange`, this._onFullscreenChange);
      if (this._hideTimer)
        clearTimeout(this._hideTimer);

      if (this._mouseIdleTimer) {
        clearTimeout(this._mouseIdleTimer);
      }
    }

    attributeChangedCallback() {
      if (this._canvas) {
        this._canvas.style.width = `${this.designWidth}px`;
        this._canvas.style.height = `${this.designHeight}px`;
        this._canvas.style.setProperty(`--deck-design-w`, `${this.designWidth}px`);
        this._canvas.style.setProperty(`--deck-design-h`, `${this.designHeight}px`);
        this._fit();
      }
    }

    _render() {
      const style = document.createElement(`style`);
      style.textContent = stylesheet;

      const stage = document.createElement(`div`);
      stage.className = `stage`;

      const canvas = document.createElement(`div`);
      canvas.className = `canvas`;
      canvas.style.width = `${this.designWidth}px`;
      canvas.style.height = `${this.designHeight}px`;
      canvas.style.setProperty(`--deck-design-w`, `${this.designWidth}px`);
      canvas.style.setProperty(`--deck-design-h`, `${this.designHeight}px`);

      const slot = document.createElement(`slot`);
      slot.addEventListener(`slotchange`, this._onSlotChange);
      canvas.appendChild(slot);
      stage.appendChild(canvas);

      const tapzones = document.createElement(`div`);
      tapzones.className = `tapzones`;
      tapzones.setAttribute(`aria-hidden`, `true`);
      const tzBack = document.createElement(`div`);
      tzBack.className = `tapzone`;
      const tzMid = document.createElement(`div`);
      tzMid.className = `tapzone`;
      tzMid.style.pointerEvents = `none`;
      const tzFwd = document.createElement(`div`);
      tzFwd.className = `tapzone`;
      tzBack.addEventListener(`click`, this._onTapBack);
      tzFwd.addEventListener(`click`, this._onTapForward);
      tapzones.append(tzBack, tzMid, tzFwd);

      const overlay = document.createElement(`div`);
      overlay.className = `overlay`;
      overlay.setAttribute(`role`, `toolbar`);
      overlay.setAttribute(`aria-label`, `Deck controls`);
      overlay.innerHTML = `
        <button class="btn prev" type="button" aria-label="Previous slide" title="Previous">
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 3L5 8l5 5"/></svg>
        </button>
        <span class="count" aria-live="polite"><span class="current">1</span><span class="sep">/</span><span class="total">1</span></span>
        <button class="btn next" type="button" aria-label="Next slide" title="Next">
          <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 3l5 5-5 5"/></svg>
        </button>
        <span class="divider"></span>
        <button class="btn reset" type="button" aria-label="Reset to first slide" title="Reset (R)">Reset<span class="kbd">R</span></button>
        <button class="btn present" type="button" aria-label="Present (fullscreen)" title="Present (P)">Present<span class="kbd">P</span></button>
      `;

      overlay.querySelector(`.prev`).addEventListener(`click`, () => this._go(this._index - 1, `click`));
      overlay.querySelector(`.next`).addEventListener(`click`, () => this._go(this._index + 1, `click`));
      overlay.querySelector(`.reset`).addEventListener(`click`, () => this._go(0, `click`));
      overlay.querySelector(`.present`).addEventListener(`click`, this._togglePresent);

      this._root.append(style, stage, tapzones, overlay);
      this._canvas = canvas;
      this._slot = slot;
      this._overlay = overlay;
      this._countEl = overlay.querySelector(`.current`);
      this._totalEl = overlay.querySelector(`.total`);
    }

    _onSlotChange() {
      this._collectSlides();
      this._restoreIndex();
      this._applyIndex({showOverlay: false, broadcast: true, reason: `init`});
      this._fit();
    }

    _collectSlides() {
      const assigned = this._slot.assignedElements({flatten: true});
      this._slides = assigned.filter(el => {
        const tag = el.tagName;
        return tag !== `TEMPLATE` && tag !== `SCRIPT` && tag !== `STYLE`;
      });

      this._slides.forEach((slide, i) => {
        slide.setAttribute(`data-deck-slide`, String(i));
      });

      const total = this._slides.length || 1;
      const totalStr = pad2(total);
      if (this._totalEl) this._totalEl.textContent = String(total);
      this._slides.forEach((slide, i) => {
        const slideStr = pad2(i + 1);
        slide.querySelectorAll(`[data-deck-slide-fill]`).forEach(el => {
          el.textContent = slideStr;
        });
        slide.querySelectorAll(`[data-deck-total-fill]`).forEach(el => {
          el.textContent = totalStr;
        });
      });
      if (this._index >= this._slides.length) {
        this._index = Math.max(0, this._slides.length - 1);
      }
    }

    _restoreIndex() {
      const h = (location.hash || ``).match(/^#(\d+)$/);
      if (h) {
        const n = parseInt(h[1], 10) - 1;
        if (n >= 0 && n < this._slides.length) {
          this._index = n;
        }
      }
    }

    _applyIndex({showOverlay = true, broadcast = true, reason = `init`} = {}) {
      if (!this._slides.length) return;
      const prev = this._prevIndex == null ? -1 : this._prevIndex;
      const curr = this._index;
      try {
        history.replaceState(null, ``, `#${curr + 1}`);
      } catch {}
      this._slides.forEach((s, i) => {
        if (i === curr) {
          s.setAttribute(`data-deck-active`, ``);
        } else {
          s.removeAttribute(`data-deck-active`);
        }
      });
      if (this._countEl) this._countEl.textContent = String(curr + 1);

      if (broadcast) {
        const detail = {
          index: curr,
          previousIndex: prev,
          total: this._slides.length,
          slide: this._slides[curr] || null,
          previousSlide: prev >= 0 ? (this._slides[prev] || null) : null,
          reason,
        };
        this.dispatchEvent(new CustomEvent(`slidechange`, {
          detail,
          bubbles: true,
          composed: true,
        }));
      }

      this._prevIndex = curr;
      if (showOverlay) {
        this._flashOverlay();
      }
    }

    _flashOverlay() {
      if (!this._overlay) return;
      if (this._isPresenting) return;
      this._overlay.setAttribute(`data-visible`, ``);
      if (this._hideTimer) clearTimeout(this._hideTimer);
      this._hideTimer = setTimeout(() => {
        this._overlay.removeAttribute(`data-visible`);
      }, OVERLAY_HIDE_MS);
    }

    _togglePresent() {
      if (document.fullscreenElement) {
        if (document.exitFullscreen) {
          document.exitFullscreen().catch(() => {});
        }
      } else {
        const el = document.documentElement;
        if (el && el.requestFullscreen) {
          el.requestFullscreen().catch(() => {});
        }
      }
    }

    _onFullscreenChange() {
      this._isPresenting = !!document.fullscreenElement;
      if (!this._overlay) return;
      if (this._isPresenting) {
        this._overlay.removeAttribute(`data-visible`);
        if (this._hideTimer) {
          clearTimeout(this._hideTimer);
          this._hideTimer = null;
        }
      }
    }

    _fit() {
      if (!this._canvas) return;
      if (this.hasAttribute(`noscale`)) {
        this._canvas.style.transform = `none`;
        return;
      }
      const vw = window.innerWidth;
      const vh = window.innerHeight;
      const s = Math.min(vw / this.designWidth, vh / this.designHeight);
      this._canvas.style.transform = `scale(${s})`;
    }

    _onResize() {
      this._fit();
    }

    _onMouseMove() {
      this._flashOverlay();
    }

    _onTapBack(e) {
      e.preventDefault();
      this._go(this._index - 1, `tap`);
    }

    _onTapForward(e) {
      e.preventDefault();
      this._go(this._index + 1, `tap`);
    }

    _onKey(e) {
      const t = e.target;
      if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      const key = e.key;
      let handled = true;

      if (key === `ArrowRight` || key === `PageDown` || key === ` ` || key === `Spacebar`) {
        this._go(this._index + 1, `keyboard`);
      } else if (key === `ArrowLeft` || key === `PageUp`) {
        this._go(this._index - 1, `keyboard`);
      } else if (key === `Home`) {
        this._go(0, `keyboard`);
      } else if (key === `End`) {
        this._go(this._slides.length - 1, `keyboard`);
      } else if (key === `r` || key === `R`) {
        this._go(0, `keyboard`);
      } else if (key === `p` || key === `P`) {
        this._togglePresent();
      } else if (/^[0-9]$/.test(key)) {
        const n = key === `0` ? 9 : parseInt(key, 10) - 1;
        if (n < this._slides.length) {
          this._go(n, `keyboard`);
        }
      } else {
        handled = false;
      }

      if (handled) {
        e.preventDefault();
        this._flashOverlay();
      }
    }

    _go(i, reason = `api`) {
      if (!this._slides.length) return;
      const clamped = Math.max(0, Math.min(this._slides.length - 1, i));
      if (clamped === this._index) {
        this._flashOverlay();
        return;
      }
      this._index = clamped;
      this._applyIndex({showOverlay: true, broadcast: true, reason});
    }

    get index() {
      return this._index;
    }
    get length() {
      return this._slides.length;
    }
    goTo(i) {
      this._go(i, `api`);
    }
    next() {
      this._go(this._index + 1, `api`);
    }
    prev() {
      this._go(this._index - 1, `api`);
    }
    reset() {
      this._go(0, `api`);
    }
  }

  if (!customElements.get(`deck-stage`)) {
    customElements.define(`deck-stage`, DeckStage);
  }
})();
