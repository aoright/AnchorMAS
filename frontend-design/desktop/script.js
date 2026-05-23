/* AnchorMAS · Desktop · script.js
 * - Tab switching (data-tab)
 * - Date picker (native input[type=date].showPicker)
 * - Region dropdown
 */

(function () {
  const $$ = (s, c) => Array.from((c || document).querySelectorAll(s));
  const app = document.querySelector('.app');

  // ===== Tabs =====
  $$('[data-tab]').forEach((el) => {
    if (el.tagName === 'BUTTON' || el.tagName === 'A') {
      el.addEventListener('click', () => {
        const tab = el.dataset.tab;
        if (!tab) return;
        app.setAttribute('data-tab', tab);
        try { localStorage.setItem('anchormas:tab', tab); } catch (_) {}
      });
    }
  });
  try {
    const saved = localStorage.getItem('anchormas:tab');
    if (saved && ['brief','chat','market','saved','settings'].includes(saved)) {
      app.setAttribute('data-tab', saved);
    }
  } catch (_) {}

  // ===== Brief state =====
  const REGIONS = {
    all: { cn: '全部市场' },
    cn:  { cn: '中国'     },
    jp:  { cn: '日本'     },
    kr:  { cn: '韩国'     },
    sea: { cn: '东南亚'   },
    us:  { cn: '美国'     },
  };
  const DOW = ['Sun','Mon','Tue','Wed','Thu','Fri','Sat'];
  const MON = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];
  function fmtDate(iso) {
    if (!iso) return '';
    const [y, m, d] = iso.split('-').map(Number);
    const dt = new Date(y, m - 1, d);
    return `${DOW[dt.getDay()]} · ${dt.getDate()} ${MON[dt.getMonth()]} ${dt.getFullYear()}`;
  }

  const state = { date: '2026-05-23', region: 'all' };
  try {
    const s = localStorage.getItem('anchormas:brief');
    if (s) Object.assign(state, JSON.parse(s));
  } catch (_) {}
  function persist() {
    try { localStorage.setItem('anchormas:brief', JSON.stringify(state)); } catch (_) {}
  }

  function render() {
    $$('[data-bind="date-text"]').forEach((el) => { el.textContent = fmtDate(state.date); });
    $$('[data-bind="date-input"]').forEach((el) => { el.value = state.date; });
    $$('[data-bind="region-text"]').forEach((el) => { el.textContent = REGIONS[state.region].cn; });
    $$('.region-picker[data-scope="brief"] .region-menu li').forEach((li) => {
      li.setAttribute('aria-selected', li.dataset.region === state.region ? 'true' : 'false');
    });
  }
  function closeMenus() {
    $$('.region-picker[data-scope="brief"] .region-menu').forEach((m) => { m.hidden = true; });
    $$('.region-picker[data-scope="brief"] [data-role="region-trigger"]').forEach((t) => { t.setAttribute('aria-expanded', 'false'); });
  }

  document.addEventListener('click', (e) => {
    const dateTrig = e.target.closest('[data-role="date-trigger"]');
    if (dateTrig) {
      const input = dateTrig.querySelector('[data-bind="date-input"]');
      if (input) {
        try { input.showPicker && input.showPicker(); }
        catch (_) { input.click(); input.focus(); }
      }
      e.stopPropagation();
      return;
    }
    const regionTrig = e.target.closest('.region-picker[data-scope="brief"] [data-role="region-trigger"]');
    if (regionTrig) {
      const menu = regionTrig.closest('.region-picker').querySelector('.region-menu');
      if (!menu) return;
      const willOpen = menu.hidden;
      closeMenus();
      if (willOpen) {
        menu.hidden = false;
        regionTrig.setAttribute('aria-expanded', 'true');
      }
      e.stopPropagation();
      return;
    }
    const opt = e.target.closest('.region-picker[data-scope="brief"] .region-menu li[data-region]');
    if (opt) {
      state.region = opt.dataset.region;
      render(); persist(); closeMenus();
      e.stopPropagation();
      return;
    }
    closeMenus();
  });

  document.addEventListener('change', (e) => {
    if (e.target.matches('[data-bind="date-input"]')) {
      state.date = e.target.value;
      render(); persist();
    }
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeMenus();
  });

  render();
})();

/* ============================================================
 * Track confirm flow (同手机端，桌面 dialog 略宽)
 * ============================================================ */
(function () {
  const STORAGE_KEY = 'anchormas:tracked';
  const dialog = document.querySelector('[data-role="track-dialog"]');
  if (!dialog) return;

  let pending = null;
  let closingTimer = null;

  function tracked() {
    try { return JSON.parse(localStorage.getItem(STORAGE_KEY)) || []; } catch (_) { return []; }
  }
  function persist(list) {
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(list)); } catch (_) {}
  }
  function findTrackBtn(storyEl) {
    return Array.from(storyEl.querySelectorAll('.story-action')).find((b) => {
      const t = b.querySelector('span')?.textContent.trim();
      return t === '追踪' || t === '已追踪';
    });
  }
  function setStoryTracked(storyEl, on) {
    if (on) storyEl.dataset.tracked = 'true';
    else delete storyEl.dataset.tracked;
    const btn = findTrackBtn(storyEl);
    if (btn) {
      const span = btn.querySelector('span');
      if (span) span.textContent = on ? '已追踪' : '追踪';
      btn.classList.toggle('is-tracked', on);
    }
  }
  function storyKey(storyEl) {
    return storyEl.querySelector('.story-headline')?.textContent.trim() || '';
  }

  function restore() {
    const list = tracked();
    document.querySelectorAll('.story').forEach((s) => {
      if (list.includes(storyKey(s))) setStoryTracked(s, true);
    });
  }
  restore();

  function openDialog(storyEl) {
    pending = storyEl;
    const headline = storyKey(storyEl) || '—';
    dialog.querySelector('[data-bind="track-headline"]').textContent = headline;
    delete dialog.dataset.state;
    dialog.hidden = false;
    document.body.style.overflow = 'hidden';
  }
  function closeDialog() {
    if (dialog.hidden) return;
    if (closingTimer) clearTimeout(closingTimer);
    dialog.dataset.state = 'closing';
    closingTimer = setTimeout(() => {
      dialog.hidden = true;
      delete dialog.dataset.state;
      document.body.style.overflow = '';
      pending = null;
      closingTimer = null;
    }, 180);
  }
  function confirmTrack() {
    if (!pending) { closeDialog(); return; }
    setStoryTracked(pending, true);
    const list = tracked();
    const key = storyKey(pending);
    if (key && !list.includes(key)) { list.push(key); persist(list); }
    closeDialog();
  }

  document.addEventListener('click', (e) => {
    if (e.target.closest('[data-role="track-cancel"]')) { closeDialog(); return; }
    if (e.target.closest('[data-role="track-confirm"]')) { confirmTrack(); return; }

    const btn = e.target.closest('.story-action');
    if (!btn) return;
    const label = btn.querySelector('span')?.textContent.trim();
    if (label !== '追踪' && label !== '已追踪') return;

    const story = btn.closest('.story');
    if (!story) return;
    e.preventDefault();
    e.stopPropagation();
    if (label === '追踪') {
      openDialog(story);
    } else {
      setStoryTracked(story, false);
      const key = storyKey(story);
      persist(tracked().filter((k) => k !== key));
    }
  }, true);

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !dialog.hidden) closeDialog();
  });
})();

/* ============================================================
 * News feed: enrich time display with relative "Xh ago"
 * ============================================================ */
(function () {
  const NOW_MIN = 12 * 60 + 30;
  function relAgo(timeStr) {
    const m = /^(\d{1,2}):(\d{2})/.exec(timeStr.trim());
    if (!m) return '';
    const itemMin = parseInt(m[1], 10) * 60 + parseInt(m[2], 10);
    const diff = NOW_MIN - itemMin;
    if (diff < 1) return 'just now';
    if (diff < 60) return `${diff}m ago`;
    return `${Math.round(diff / 60)}h ago`;
  }
  document.querySelectorAll('.news-time').forEach((t) => {
    if (t.nextElementSibling && t.nextElementSibling.classList.contains('news-rel')) return;
    const rel = relAgo(t.textContent);
    if (!rel) return;
    const span = document.createElement('span');
    span.className = 'news-rel';
    span.textContent = rel;
    t.after(span);
  });
})();

/* ============================================================
 * News feed: region filter — driven by sidebar region picker
 *   (Desktop: 没有 inline pills；sidebar 里的 data-scope="news" picker 控制)
 * ============================================================ */
(function () {
  const $$ = (s) => Array.from(document.querySelectorAll(s));
  const view = document.querySelector('.news-view');
  if (!view) return;

  const REGIONS = {
    all: '全部市场', cn: '中国', jp: '日本',
    kr: '韩国', sea: '东南亚', us: '美国',
  };

  let region = 'all';
  try {
    const saved = localStorage.getItem('anchormas:news-region');
    if (saved && saved in REGIONS) region = saved;
  } catch (_) {}

  function render() {
    view.setAttribute('data-active-region', region);
    $$('[data-bind="news-region-text"]').forEach((el) => { el.textContent = REGIONS[region]; });
    $$('.region-picker[data-scope="news"] .region-menu li').forEach((li) => {
      li.setAttribute('aria-selected', li.dataset.region === region ? 'true' : 'false');
    });
  }
  function persist() {
    try { localStorage.setItem('anchormas:news-region', region); } catch (_) {}
  }
  function closeMenu() {
    const m = document.querySelector('.region-picker[data-scope="news"] .region-menu');
    if (m) m.hidden = true;
    const t = document.querySelector('[data-role="news-region-trigger"]');
    if (t) t.setAttribute('aria-expanded', 'false');
  }

  document.addEventListener('click', (e) => {
    const trig = e.target.closest('[data-role="news-region-trigger"]');
    if (trig) {
      const menu = trig.closest('.region-picker')?.querySelector('.region-menu');
      if (!menu) return;
      const willOpen = menu.hidden;
      closeMenu();
      if (willOpen) {
        menu.hidden = false;
        trig.setAttribute('aria-expanded', 'true');
      }
      e.stopPropagation();
      return;
    }
    const opt = e.target.closest('.region-picker[data-scope="news"] .region-menu li[data-region]');
    if (opt) {
      region = opt.dataset.region;
      render(); persist(); closeMenu();
      e.stopPropagation();
      return;
    }
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeMenu();
  });

  render();
})();

/* ============================================================
 * Source viewer (right canvas)
 * - 从 story-sources 链接 + news-item 都能打开
 * ============================================================ */
(function () {
  const $$ = (s) => Array.from(document.querySelectorAll(s));
  const viewer = document.querySelector('[data-role="source-canvas"]');
  if (!viewer) return;

  function populate({ name, time, title }) {
    $$('[data-bind="source-name"]').forEach((el) => { el.textContent = name; });
    $$('[data-bind="source-time"]').forEach((el) => {
      el.textContent = time ? `${time} · 23 May 2026` : '23 May 2026';
    });
    $$('[data-bind="source-title"]').forEach((el) => { el.textContent = title; });
  }

  function show() {
    delete viewer.dataset.state;
    viewer.hidden = false;
    document.body.style.overflow = 'hidden';
  }
  function openFromStoryLink(linkEl) {
    const story = linkEl.closest('.story');
    // 优先用 source 自己的 title（data-source-title）；否则 fallback 到 story headline
    const sourceTitle = linkEl.dataset.sourceTitle?.trim();
    const fallback   = story?.querySelector('.story-headline')?.textContent.trim() || '';
    populate({
      title: sourceTitle || fallback,
      name: linkEl.querySelector('.src-name')?.textContent.trim() || 'Source',
      time: linkEl.querySelector('time')?.textContent.trim() || '',
    });
    show();
  }
  function openFromNewsItem(itemEl) {
    populate({
      title: itemEl.querySelector('.news-headline')?.textContent.trim() || '',
      name: itemEl.querySelector('.news-source')?.textContent.trim() || 'Source',
      time: itemEl.querySelector('.news-time')?.textContent.trim() || '',
    });
    show();
  }
  let closingTimer = null;
  function close() {
    if (viewer.hidden) return;
    if (closingTimer) clearTimeout(closingTimer);
    viewer.dataset.state = 'closing';
    closingTimer = setTimeout(() => {
      viewer.hidden = true;
      delete viewer.dataset.state;
      document.body.style.overflow = '';
      closingTimer = null;
    }, 280);
  }

  document.addEventListener('click', (e) => {
    const link = e.target.closest('.story-sources a');
    if (link) { e.preventDefault(); openFromStoryLink(link); return; }

    const newsItem = e.target.closest('.news-item');
    if (newsItem) { openFromNewsItem(newsItem); return; }

    if (e.target.closest('[data-role="source-close"]')) { close(); }
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !viewer.hidden) close();
  });
})();
