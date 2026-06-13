/* ────────── "Do you know Yarn?" quiz logic ────────── */
/* QUESTIONS and LEVELS are injected at build time from config/quiz.json */

/* ────────── State ────────── */
var state = {
  order: [],
  cursor: 0,
  answers: {},
  startedFromSlug: null,
};

/* ────────── Utilities ────────── */
function shuffle(arr) {
  var a = arr.slice();
  for (var i = a.length - 1; i > 0; i--) {
    var j = Math.floor(Math.random() * (i + 1));
    var tmp = a[i]; a[i] = a[j]; a[j] = tmp;
  }
  return a;
}

function slugToIndex(slug) {
  return QUESTIONS.findIndex(q => {
    return q.slug === slug;
  });
}

function buildOrder() {
  var hash = (location.hash || ``).replace(/^#/, ``).trim();
  var allIdx = QUESTIONS.map((_, i) => {
    return i;
  });
  if (hash) {
    var startIdx = slugToIndex(hash);
    if (startIdx >= 0) {
      state.startedFromSlug = hash;
      var rest = shuffle(allIdx.filter(i => {
        return i !== startIdx;
      }));
      return [startIdx].concat(rest);
    }
  }
  return allIdx;
}

function copyToClipboard(text) {
  if (navigator.clipboard && navigator.clipboard.writeText)
    return navigator.clipboard.writeText(text);

  var ta = document.createElement(`textarea`);
  ta.value = text;
  ta.style.position = `fixed`;
  ta.style.opacity = `0`;
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand(`copy`);
  } catch {}
  document.body.removeChild(ta);
  return Promise.resolve();
}

/* ────────── Rendering ────────── */
var stage, shell, progressFill, progressNum, scoreNum, progressTotal;

function updateProgress() {
  var total = state.order.length;
  var answered = Object.keys(state.answers).length;
  var correct = 0;
  for (var k in state.answers) if (state.answers[k].correct) correct++;
  progressNum.textContent = Math.min(state.cursor + 1, total);
  progressTotal.textContent = total;
  scoreNum.textContent = correct;
  var pct = (answered / total) * 100;
  progressFill.style.width = `${pct}%`;
  if (shell) {
    var started = answered > 0 || state.cursor > 0;
    shell.classList.toggle(`compact`, started);
  }
}

function renderQuestion() {
  updateProgress();
  var total = state.order.length;
  if (state.cursor >= total) {
    renderEnd();
    return;
  }
  var q = QUESTIONS[state.order[state.cursor]];
  history.replaceState(null, ``, `#${q.slug}`);

  var already = state.answers[q.slug];

  var content = document.createElement(`div`);
  content.className = `quiz-stage`;
  content.innerHTML =
    `<div class="q-head">` +
      `<div class="q-prompt-col">` +
        `<div class="q-number">Question ${state.cursor + 1} of ${total}</div>` +
        `<h2 class="q-prompt">${q.question}</h2>` +
      `</div>` +
      `<div class="q-answers" role="group" aria-label="Answer">` +
        `<button class="q-btn" data-answer="true">` +
          `<span>Yes</span>${
            answerIcons()
          }</button>` +
        `<button class="q-btn" data-answer="false">` +
          `<span>No</span>${
            answerIcons()
          }</button>` +
      `</div>` +
    `</div>` +
    `<div class="q-reveal" id="reveal" aria-live="polite"></div>`;
  stage.replaceChildren(content);

  var buttons = content.querySelectorAll(`.q-btn`);
  buttons.forEach(btn => {
    btn.addEventListener(`click`, () => {
      btn.classList.add(`pulse`);
      handleAnswer(q, btn.dataset.answer === `true`);
    });
  });

  if (already) {
    applyAnswerUI(content, q, already.picked);
  }
}

function answerIcons() {
  return `<svg class="q-icon q-icon-check" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 10.5 8.5 15 16 6"/></svg>` +
    `<svg class="q-icon q-icon-cross" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 5 15 15M15 5 5 15"/></svg>`;
}

function applyAnswerUI(root, q, picked) {
  var correct = picked === q.answer;
  var buttons = root.querySelectorAll(`.q-btn`);
  buttons.forEach(b => {
    b.disabled = true;
    var btnAnswer = b.dataset.answer === `true`;
    var isPicked = btnAnswer === picked;
    var isCorrectAnswer = btnAnswer === q.answer;
    if (isPicked) {
      b.classList.add(`picked`, correct ? `correct` : `wrong`);
    } else if (isCorrectAnswer) {
      b.classList.add(`revealed-correct`);
    }
  });

  var line = correct ? q.rightLine : q.wrongLine;
  var verdictLabel = correct ? `Correct` : `Not quite`;
  var verdictClass = correct ? `right` : `wrong`;

  var reveal = root.querySelector(`#reveal`);
  reveal.innerHTML =
    `<div class="q-verdict ${verdictClass}">${
      dotIcon(verdictClass)
    }<span>${verdictLabel}</span>` +
    `</div>` +
    `<p class="q-verdict-line">${line}</p>${
      q.explain.map(p => {
        return `<p class="q-explain">${p}</p>`;
      }).join(``)
    }<div class="q-actions">` +
      `<button class="q-next" id="next-btn">${
        state.cursor + 1 >= state.order.length ? `See results` : `Next question`
      } <svg viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7h8M8 4l3 3-3 3"/></svg>` +
      `</button>` +
      `<button class="q-share" id="share-btn" aria-label="Copy link to this question">` +
        `<svg viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5.5 8.5 L8.5 5.5 M6 4h2.5a2 2 0 0 1 0 4H7 M7 10H5.5a2 2 0 0 1 0-4H6"/></svg>` +
        `<span id="share-label">Share question</span>` +
      `</button>` +
    `</div>`;
  reveal.classList.add(`open`);

  reveal.querySelector(`#next-btn`).addEventListener(`click`, advance);
  var shareBtn = reveal.querySelector(`#share-btn`);
  shareBtn.addEventListener(`click`, () => {
    var url = `${location.origin + location.pathname}#${q.slug}`;
    copyToClipboard(url).then(() => {
      shareBtn.classList.add(`copied`);
      reveal.querySelector(`#share-label`).textContent = `Link copied`;
      setTimeout(() => {
        shareBtn.classList.remove(`copied`);
        var lbl = reveal.querySelector(`#share-label`);
        if (lbl) {
          lbl.textContent = `Share question`;
        }
      }, 1800);
    });
  });
}

function dotIcon(kind) {
  var color = kind === `right` ? `var(--accent)` : `var(--fg-mute)`;
  return `<span style="display:inline-block;width:7px;height:7px;border-radius:50%;background:${color}"></span>`;
}

function handleAnswer(q, picked) {
  if (state.answers[q.slug]) return;
  var correct = picked === q.answer;
  state.answers[q.slug] = {picked, correct};
  updateProgress();
  applyAnswerUI(stage.querySelector(`.quiz-stage`), q, picked);
}

function advance() {
  state.cursor += 1;
  renderQuestion();
  requestAnimationFrame(() => {
    window.scrollTo({top: 0, behavior: `smooth`});
  });
}

/* ────────── End screen ────────── */
function renderEnd() {
  history.replaceState(null, ``, `#results`);
  var total = state.order.length;
  var correct = 0;
  for (var k in state.answers) if (state.answers[k].correct) correct++;
  var level = LEVELS[0];
  for (var i = LEVELS.length - 1; i >= 0; i--) {
    if (correct >= LEVELS[i].min) {
      level = LEVELS[i]; break;
    }
  }


  var recap = state.order.map(idx => {
    var q = QUESTIONS[idx];
    var a = state.answers[q.slug];
    return `<a class="recap-row" href="#${q.slug}" data-slug="${q.slug}">` +
      `<span class="recap-dot ${a && a.correct ? `right` : ``}"></span>` +
      `<span class="recap-text">${q.question}</span>` +
      `<svg class="recap-arrow" width="14" height="14" viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7h8M8 4l3 3-3 3"/></svg>` +
    `</a>`;
  }).join(``);

  var content = document.createElement(`div`);
  content.className = `quiz-stage`;
  content.innerHTML =
    `<div class="end-screen">` +
      `<div class="end-level-label">Your Yarn level</div>` +
      `<h2 class="end-level">${level.title}</h2>` +
      `<div class="end-score"><span class="num">${correct}</span><span class="total"> / ${total} correct</span></div>` +
      `<p class="end-tagline">${level.tag}</p>` +
      `<div class="end-actions">` +
        `<button class="q-next" id="restart-btn">` +
          `Play again` +
          ` <svg viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 6A5 5 0 1 0 11.5 9.5 M12 3v3h-3"/></svg>` +
        `</button>` +
        `<button class="q-share" id="share-score-btn">` +
          `<svg viewBox="0 0 14 14" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5.5 8.5 L8.5 5.5 M6 4h2.5a2 2 0 0 1 0 4H7 M7 10H5.5a2 2 0 0 1 0-4H6"/></svg>` +
          `<span id="share-score-label">Copy shareable link</span>` +
        `</button>` +
      `</div>` +
      `<div class="end-recap">` +
        `<div class="end-recap-title">Your answers \u2014 tap to revisit a question</div>${
          recap
        }</div>` +
    `</div>`;
  stage.replaceChildren(content);

  document.getElementById(`restart-btn`).addEventListener(`click`, restart);
  var sb = document.getElementById(`share-score-btn`);
  sb.addEventListener(`click`, () => {
    var url = location.origin + location.pathname;
    copyToClipboard(url).then(() => {
      sb.classList.add(`copied`);
      document.getElementById(`share-score-label`).textContent = `Link copied`;
      setTimeout(() => {
        sb.classList.remove(`copied`);
        var lbl = document.getElementById(`share-score-label`);
        if (lbl) {
          lbl.textContent = `Copy shareable link`;
        }
      }, 1800);
    });
  });

  content.querySelectorAll(`.recap-row`).forEach(row => {
    row.addEventListener(`click`, e => {
      e.preventDefault();
      var slug = row.dataset.slug;
      var pos = state.order.findIndex(i => {
        return QUESTIONS[i].slug === slug;
      });
      if (pos >= 0) {
        state.cursor = pos;
        renderQuestion();
        requestAnimationFrame(() => {
          window.scrollTo({top: 0, behavior: `smooth`});
        });
      }
    });
  });
}

function restart() {
  state.answers = {};
  state.cursor = 0;
  state.order = shuffle(QUESTIONS.map((_, i) => {
    return i;
  }));
  history.replaceState(null, ``, location.pathname);
  renderQuestion();
  requestAnimationFrame(() => {
    window.scrollTo({top: 0, behavior: `smooth`});
  });
}

/* ────────── Init ────────── */
function quizInit() {
  stage = document.getElementById(`stage`);
  shell = document.querySelector(`.quiz-shell`);
  progressFill = document.getElementById(`progress-fill`);
  progressNum = document.getElementById(`progress-num`);
  scoreNum = document.getElementById(`score-num`);
  progressTotal = document.getElementById(`progress-total`);

  if (!stage) return;
  state.order = buildOrder();
  renderQuestion();
}

document.addEventListener(`DOMContentLoaded`, quizInit);
