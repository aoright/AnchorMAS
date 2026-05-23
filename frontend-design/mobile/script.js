/* AnchorMAS · Mobile · script.js
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

  // ===== Brief state (date + region) =====
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
    $$('.region-menu li').forEach((li) => {
      li.setAttribute('aria-selected', li.dataset.region === state.region ? 'true' : 'false');
    });
  }

  function closeMenus() {
    $$('.region-menu').forEach((m) => { m.hidden = true; });
    $$('[data-role="region-trigger"]').forEach((t) => { t.setAttribute('aria-expanded', 'false'); });
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

    const regionTrig = e.target.closest('[data-role="region-trigger"]');
    if (regionTrig) {
      const menu = regionTrig.closest('.region-picker')?.querySelector('.region-menu');
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

    const opt = e.target.closest('.region-menu li[data-region]');
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
 * Story more-toggle (折叠 sources/收藏/追问)
 * - 点击 .story-more 切换
 * - 长按 .story 本体 450ms 也切换
 * ============================================================ */
(function () {
  function toggleFoot(story, force) {
    const foot = story.querySelector('.story-foot');
    const btn  = story.querySelector('.story-more');
    if (!foot) return;
    const willOpen = typeof force === 'boolean' ? force : foot.hidden;
    foot.hidden = !willOpen;
    if (btn) btn.setAttribute('aria-expanded', willOpen ? 'true' : 'false');
  }

  document.addEventListener('click', (e) => {
    const trig = e.target.closest('.story-more');
    if (trig) {
      const story = trig.closest('.story');
      if (story) toggleFoot(story);
    }
  });

  // 长按检测
  let pressTimer = null;
  let pressedStory = null;
  document.addEventListener('touchstart', (e) => {
    const story = e.target.closest('.story');
    if (!story) return;
    if (e.target.closest('a, button')) return;     // 避免和 link/button 冲突
    pressedStory = story;
    pressTimer = setTimeout(() => {
      if (pressedStory) toggleFoot(pressedStory);
      pressTimer = null;
    }, 450);
  }, { passive: true });
  ['touchend', 'touchcancel', 'touchmove'].forEach((ev) => {
    document.addEventListener(ev, () => {
      if (pressTimer) { clearTimeout(pressTimer); pressTimer = null; }
      pressedStory = null;
    }, { passive: true });
  });
})();

/* ============================================================
 * News feed: enrich time display with relative "Xh ago"
 *   (假设"现在"是 12:30 PM；真实场景里用 Date.now() 算)
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
 * News feed: region filter + item click → source viewer
 * ============================================================ */
(function () {
  const view = document.querySelector('.news-view');
  if (!view) return;

  document.addEventListener('click', (e) => {
    const pill = e.target.closest('.news-pill');
    if (!pill) return;
    const region = pill.dataset.region;
    view.setAttribute('data-active-region', region);
    document.querySelectorAll('.news-pill').forEach((p) => {
      p.classList.toggle('is-active', p === pill);
    });
  });
})();

/* ============================================================
 * Source viewer (bottom sheet)
 * - 从 story-sources 链接打开（简报页）
 * - 从 news-item 整条打开（新闻页）
 * ============================================================ */
(function () {
  const $$ = (s) => Array.from(document.querySelectorAll(s));
  const viewer = document.querySelector('[data-role="source-sheet"]');
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

  function openFromStoryLink(linkEl) {
    const story = linkEl.closest('.story');
    populate({
      title: story?.querySelector('.story-headline')?.textContent.trim() || '',
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
