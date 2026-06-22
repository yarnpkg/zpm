(function () {
  function toast(msg) {
    let t = document.querySelector(`.blog-toast`);
    if (!t) {
      t = document.createElement(`div`);
      t.className = `blog-toast`;
      t.setAttribute(`role`, `status`);
      Object.assign(t.style, {
        position: `fixed`, left: `50%`, bottom: `28px`,
        transform: `translateX(-50%) translateY(10px)`,
        padding: `10px 16px`, borderRadius: `10px`,
        background: `color-mix(in oklch, var(--bg-0) 80%, transparent)`,
        border: `1px solid var(--line-strong)`, color: `var(--fg)`,
        fontSize: `13px`, fontFamily: `inherit`, zIndex: 100,
        backdropFilter: `blur(10px)`, opacity: `0`,
        transition: `opacity 0.2s, transform 0.2s`, pointerEvents: `none`,
      });
      document.body.appendChild(t);
    }
    t.textContent = msg;
    requestAnimationFrame(() => {
      t.style.opacity = `1`;
      t.style.transform = `translateX(-50%) translateY(0)`;
    });
    clearTimeout(t._timer);
    t._timer = setTimeout(() => {
      t.style.opacity = `0`;
      t.style.transform = `translateX(-50%) translateY(10px)`;
    }, 1600);
  }

  var prose = document.querySelector(`.article-prose`);
  if (!prose) return;

  var headings = Array.from(prose.querySelectorAll(`h2, h3`));
  headings.forEach(h => {
    var anchor = h.querySelector(`.heading-anchor`);
    if (anchor) {
      anchor.addEventListener(`click`, e => {
        e.preventDefault();
        var url = `${location.origin + location.pathname}#${h.id}`;
        history.replaceState(null, ``, `#${h.id}`);
        if (navigator.clipboard) {
          navigator.clipboard.writeText(url).then(
            () => {
              toast(`Link copied`);
            },
            () => {
              toast(`Press \u2318C to copy`);
            },
          );
        }
      });
    }
  });

  var toc = document.querySelector(`.toc`);
  if (toc) {
    var h2s = headings.filter(h => {
      return h.tagName === `H2`;
    });
    var links = Array.from(toc.querySelectorAll(`a`));
    function onScroll() {
      var y = window.scrollY + 140;
      var activeId = h2s[0].id;
      for (var i = 0; i < h2s.length; i++)
        if (h2s[i].offsetTop <= y) activeId = h2s[i].id;

      links.forEach(l => {
        l.classList.toggle(`active`, l.getAttribute(`href`) === `#${activeId}`);
      });
    }
    window.addEventListener(`scroll`, onScroll, {passive: true});
    onScroll();
  }

  document.querySelectorAll(`[data-share="copy-url"]`).forEach(btn => {
    btn.addEventListener(`click`, e => {
      e.preventDefault();
      var url = location.href;
      if (navigator.clipboard) {
        navigator.clipboard.writeText(url).then(
          () => {
            toast(`Link copied`);
          },
          () => {
            toast(`Press \u2318C to copy`);
          },
        );
      }
    });
  });
})();
