/* AnchorMAS · Desktop · script.js
 * - Tab switching (data-tab)
 * - Date picker (native input[type=date].showPicker)
 * - Region dropdown
 */

/* Theme restore — 必须最早跑，避免页面初次闪白/闪暗 */
(function () {
  try {
    const saved = localStorage.getItem('anchormas:settings:theme');
    if (saved === 'dark' || saved === 'light') {
      document.documentElement.dataset.theme = saved;
    }
  } catch (_) {}
})();

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
 * Settings: multi-zone clock + lang + checkboxes 持久化（同手机端）
 * ============================================================ */
(function () {
  const ZONES = { cn: 0, jp: 1, kr: 1, sea: -1, us: -16 };
  const PILL_PERSIST = {
    'markets-checks': 'anchormas:settings:markets',
    'dims-checks':    'anchormas:settings:dimensions',
  };
  const LANG_KEY = 'anchormas:settings:lang';
  const TIME_KEY = 'anchormas:settings:push-time';

  function pad(n) { return String(n).padStart(2, '0'); }
  function computeClock(baseHHMM) {
    const [bh, bm] = baseHHMM.split(':').map(Number);
    if (Number.isNaN(bh) || Number.isNaN(bm)) return null;
    const baseMin = bh * 60 + bm;
    const out = {};
    for (const [zone, offHours] of Object.entries(ZONES)) {
      let t = baseMin + offHours * 60;
      let day = 0;
      while (t < 0)        { t += 24 * 60; day -= 1; }
      while (t >= 24 * 60) { t -= 24 * 60; day += 1; }
      out[zone] = { h: Math.floor(t / 60), m: t % 60, day };
    }
    return out;
  }
  function renderClock(base) {
    const out = computeClock(base);
    if (!out) return;
    for (const [zone, v] of Object.entries(out)) {
      const el = document.querySelector(`[data-bind="clock-${zone}"]`);
      if (!el) continue;
      const dayTag = v.day === -1 ? '<span class="day-shift">前一日</span>'
                  : v.day === +1 ? '<span class="day-shift">次日</span>'
                  : '';
      el.innerHTML = `${pad(v.h)}:${pad(v.m)} ${dayTag}`;
    }
  }

  // —— Push time (custom 2-segment picker) ——
  const picker = document.querySelector('[data-role="time-picker"]');
  if (picker) {
    const VALID_MIN = ['00', '15', '30', '45'];
    const hourMenu = picker.querySelector('[data-segment="hour"] .time-menu');
    const minMenu  = picker.querySelector('[data-segment="minute"] .time-menu');
    const hourText = picker.querySelector('[data-bind="hour-text"]');
    const minText  = picker.querySelector('[data-bind="minute-text"]');

    if (hourMenu && !hourMenu.children.length) {
      for (let h = 0; h < 24; h++) {
        const li = document.createElement('li');
        li.setAttribute('role', 'option');
        li.dataset.value = pad(h);
        li.textContent = pad(h);
        hourMenu.appendChild(li);
      }
    }

    const state = { hour: '08', minute: '00' };
    try {
      const saved = localStorage.getItem(TIME_KEY);
      if (saved && /^\d{2}:\d{2}$/.test(saved)) {
        const [h, m] = saved.split(':');
        state.hour = h;
        state.minute = VALID_MIN.includes(m) ? m : '00';
      }
    } catch (_) {}

    function syncSelected() {
      hourMenu?.querySelectorAll('li').forEach((li) => {
        li.setAttribute('aria-selected', li.dataset.value === state.hour ? 'true' : 'false');
      });
      minMenu?.querySelectorAll('li').forEach((li) => {
        li.setAttribute('aria-selected', li.dataset.value === state.minute ? 'true' : 'false');
      });
    }

    function applyState() {
      if (hourText) hourText.textContent = state.hour;
      if (minText)  minText.textContent  = state.minute;
      syncSelected();
      renderClock(`${state.hour}:${state.minute}`);
      try { localStorage.setItem(TIME_KEY, `${state.hour}:${state.minute}`); } catch (_) {}
    }
    applyState();

    function closeTimeMenus() {
      picker.querySelectorAll('.time-menu').forEach((m) => { m.hidden = true; });
      picker.querySelectorAll('[aria-haspopup="listbox"]').forEach((t) => t.setAttribute('aria-expanded', 'false'));
    }

    document.addEventListener('click', (e) => {
      const trig = e.target.closest('[data-role="hour-trigger"], [data-role="minute-trigger"]');
      if (trig && picker.contains(trig)) {
        const wrap = trig.closest('.time-seg-wrap');
        const menu = wrap?.querySelector('.time-menu');
        if (!menu) return;
        const willOpen = menu.hidden;
        closeTimeMenus();
        if (willOpen) {
          menu.hidden = false;
          trig.setAttribute('aria-expanded', 'true');
          const sel = menu.querySelector('li[aria-selected="true"]');
          if (sel) sel.scrollIntoView({ block: 'nearest' });
        }
        e.stopPropagation();
        return;
      }
      const opt = e.target.closest('.time-menu li[data-value]');
      if (opt && picker.contains(opt)) {
        const seg = opt.closest('.time-seg-wrap')?.dataset.segment;
        if (seg === 'hour')   state.hour   = opt.dataset.value;
        if (seg === 'minute') state.minute = opt.dataset.value;
        applyState();
        closeTimeMenus();
        e.stopPropagation();
        return;
      }
      if (!e.target.closest('[data-role="time-picker"]')) closeTimeMenus();
    });

    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') closeTimeMenus();
    });
  }

  function bindSegControl({ role, key, attr, apply }) {
    const group = document.querySelector(`[data-role="${role}"]`);
    if (!group) return;
    try {
      const saved = localStorage.getItem(key);
      if (saved) {
        group.querySelectorAll('.seg-btn').forEach((b) => {
          b.classList.toggle('is-active', b.dataset[attr] === saved);
        });
        if (apply) apply(saved);
      }
    } catch (_) {}
    group.addEventListener('click', (e) => {
      const btn = e.target.closest('.seg-btn');
      if (!btn) return;
      const value = btn.dataset[attr];
      group.querySelectorAll('.seg-btn').forEach((b) => {
        b.classList.toggle('is-active', b === btn);
      });
      try { localStorage.setItem(key, value); } catch (_) {}
      if (apply) apply(value);
    });
  }

  bindSegControl({
    role: 'theme-toggle',
    key:  'anchormas:settings:theme',
    attr: 'theme',
    apply: (v) => { document.documentElement.dataset.theme = v; },
  });

  bindSegControl({
    role: 'lang-toggle',
    key:  LANG_KEY,
    attr: 'lang',
  });

  bindSegControl({
    role: 'max-stories-toggle',
    key:  'anchormas:settings:max-stories',
    attr: 'value',
  });

  for (const [role, key] of Object.entries(PILL_PERSIST)) {
    const group = document.querySelector(`[data-role="${role}"]`);
    if (!group) continue;
    try {
      const raw = localStorage.getItem(key);
      if (raw) {
        const selected = JSON.parse(raw);
        group.querySelectorAll('input[type="checkbox"]').forEach((cb) => {
          cb.checked = selected.includes(cb.value);
        });
      }
    } catch (_) {}
    group.addEventListener('change', () => {
      const selected = Array.from(group.querySelectorAll('input:checked')).map((c) => c.value);
      try { localStorage.setItem(key, JSON.stringify(selected)); } catch (_) {}
    });
  }
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

  // 来自 chat 的 citation 点击通过自定义事件触发
  document.addEventListener('anchormas:open-source', (e) => {
    const d = e.detail || {};
    populate({ name: d.name || 'Source', time: d.time || '', title: d.title || '' });
    show();
  });
})();


/* ============================================================
 * Chat — sessions, mock replies (桌面无 chrome / 无 sheet)
 * ============================================================ */
(function () {
  const $$ = (s, ctx) => Array.from((ctx || document).querySelectorAll(s));
  const $  = (s, ctx) => (ctx || document).querySelector(s);

  const SESSIONS_KEY = 'anchormas:chat:sessions';
  const CURRENT_KEY  = 'anchormas:chat:current-session-id';

  const PRESETS = {
    '今日哪个市场风险最高？': {
      text: '根据今日 5 个市场扫描，**东南亚（越南）** 风险最高 — Severity 5 / Urgency 4。TikTok Shop 越南站抽佣由 5% 上调至 8%，6 月起执行，直接挤压本地化定价空间。[1][2]\n\n次高是 **中国** （Severity 4 / Urgency 4），海关跨境电商 HS 编码申报抽查升级。[3]\n\n建议本周优先处理这两条。',
      citations: [
        { num: 1, name: 'Reuters',   time: '03:42', title: 'TikTok Shop Vietnam to raise commission rate from 5% to 8%' },
        { num: 2, name: 'VnExpress', time: '02:18', title: 'TikTok Shop Việt Nam tăng phí hoa hồng' },
        { num: 3, name: '海关总署',  time: '08:12', title: '海关总署关于跨境电商出口 HS 编码申报抽查规则升级的公告' }
      ]
    },
    '对比中日韩本周变动': {
      text: '三国当前一致信号：**平台 / 法规驱动**变动。\n\n**中国** · Risk · 海关 HS 编码抽查升级，影响中小品牌清关速度。[1]\n**日本** · Attention · 消費者庁 EC 标价规范修订意见征集，营销文案合规需检视。[2]\n**韩国** · Opportunity · Olive Young × 7-Eleven 渠道扩张，K-Beauty 同品类有进入窗口。[3]\n\n建议团队带宽优先压在中国清关 + 日本表达合规上；韩国机会窗口可由 BD 单线推进。',
      citations: [
        { num: 1, name: '海关总署',         time: '08:12', title: '海关总署 HS 编码抽查公告' },
        { num: 2, name: '消費者庁',         time: '08:00', title: '電子商取引における価格表示の適正化' },
        { num: 3, name: 'Olive Young 公告', time: '06:00', title: '올리브영 × 7-Eleven 채널 확장 공식 안내' }
      ]
    },
    '越南那条对我品类影响多大？': {
      text: '假设你做美妆 $5-15 价位 SKU：\n\n3% 抽佣上调 ≈ 单 SKU 毛利下降 **4-6%**。如果靠跑量摊薄边际，影响可控；如果依赖小批量高频上新，毛利会更敏感。\n\n短期建议：\n• Hero SKU 定价上调 **4-6%**，吸收增量抽佣\n• 自然流主推 + 减少站内付费占比\n• 6 月前完成本地化定价测试，避免新规生效首日库存空档[1]',
      citations: [
        { num: 1, name: 'Reuters', time: '03:42', title: 'TikTok Shop Vietnam to raise commission rate from 5% to 8%' }
      ]
    }
  };
  function pickAnswer(q) {
    return PRESETS[q.trim()] || {
      text: '演示模式：尚未接入实时检索。\n\n试试这几个预设问题：\n• 今日哪个市场风险最高？\n• 对比中日韩本周变动\n• 越南那条对我品类影响多大？',
      citations: []
    };
  }

  function loadSessions() { try { return JSON.parse(localStorage.getItem(SESSIONS_KEY)) || []; } catch (_) { return []; } }
  function saveSessions(arr) { try { localStorage.setItem(SESSIONS_KEY, JSON.stringify(arr)); } catch (_) {} }
  function getCurrentId() { try { return localStorage.getItem(CURRENT_KEY); } catch (_) { return null; } }
  function setCurrentId(id) {
    try {
      if (id) localStorage.setItem(CURRENT_KEY, id);
      else localStorage.removeItem(CURRENT_KEY);
    } catch (_) {}
  }
  function findSession(id) { return id ? loadSessions().find(s => s.id === id) : null; }
  function upsertSession(s) {
    const all = loadSessions();
    const i = all.findIndex(x => x.id === s.id);
    if (i === -1) all.unshift(s);
    else all[i] = s;
    saveSessions(all);
  }
  function newSession(opts = {}) {
    const id = 'sess-' + Date.now() + '-' + Math.random().toString(36).slice(2, 8);
    const s = {
      id,
      title: opts.title || '新对话',
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
      context_story_id: opts.context_story_id || null,
      context_text: opts.context_text || null,
      messages: opts.messages || []
    };
    upsertSession(s);
    setCurrentId(id);
    return s;
  }
  function addMessage(sid, role, content, citations) {
    const s = findSession(sid);
    if (!s) return;
    s.messages.push({ role, content, ts: new Date().toISOString(), citations: citations || null });
    s.updated_at = new Date().toISOString();
    if (role === 'user' && s.messages.filter(m => m.role === 'user').length === 1 && (s.title === '新对话' || !s.context_text)) {
      s.title = content.slice(0, 28) + (content.length > 28 ? '…' : '');
    }
    upsertSession(s);
  }

  function pad(n) { return String(n).padStart(2, '0'); }
  function fmtTime(iso) { const d = new Date(iso); return pad(d.getHours()) + ':' + pad(d.getMinutes()); }
  function fmtRelTime(iso) {
    const d = new Date(iso);
    const diffMin = Math.floor((Date.now() - d.getTime()) / 60000);
    if (diffMin < 1) return 'just now';
    if (diffMin < 60) return diffMin + 'm';
    if (diffMin < 1440) return Math.floor(diffMin / 60) + 'h';
    if (diffMin < 10080) return Math.floor(diffMin / 1440) + 'd';
    return (d.getMonth() + 1) + '/' + d.getDate();
  }
  function escapeHtml(s) {
    return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
                    .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }
  function renderMarkdown(text) {
    let html = escapeHtml(text);
    html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
    html = html.replace(/\[(\d+)\]/g, '<sup class="cite" data-ref="$1">[$1]</sup>');
    html = html.split('\n\n').map(p => '<p>' + p.replace(/\n/g, '<br>') + '</p>').join('');
    return html;
  }
  function renderCitationsBlock(citations) {
    if (!citations || !citations.length) return '';
    const items = citations.map(c =>
      '<button class="chat-citation" type="button"' +
      ' data-cite-num="' + c.num + '"' +
      ' data-cite-name="' + escapeHtml(c.name) + '"' +
      ' data-cite-time="' + c.time + '"' +
      ' data-cite-title="' + escapeHtml(c.title || '') + '">' +
        '<span class="cite-num">[' + c.num + ']</span>' +
        '<span class="cite-name">' + escapeHtml(c.name) + '</span>' +
        '<time>' + c.time + '</time>' +
      '</button>'
    ).join('');
    return '<div class="chat-citations">' + items + '</div>';
  }
  function renderBlock(m) {
    const el = document.createElement('div');
    if (m.role === 'user') {
      el.className = 'chat-block chat-block-user';
      el.innerHTML =
        '<div class="chat-meta"><span class="chat-role">You</span><time>' + fmtTime(m.ts) + '</time></div>' +
        '<p class="chat-bubble">' + escapeHtml(m.content) + '</p>';
    } else {
      el.className = 'chat-block chat-block-agent';
      el.innerHTML =
        '<div class="chat-meta"><span class="chat-role">AnchorMAS</span><time>' + fmtTime(m.ts) + '</time></div>' +
        '<div class="chat-reply">' + renderMarkdown(m.content) + '</div>' +
        renderCitationsBlock(m.citations);
    }
    return el;
  }

  function renderFeed() {
    const feed  = $('[data-bind="chat-feed"]');
    const empty = $('[data-bind="chat-empty"]');
    const title = $('[data-bind="chat-title"]');
    const ctxEl = $('[data-bind="chat-context"]');
    const ctxText = $('[data-bind="chat-context-text"]');

    const s = findSession(getCurrentId());
    if (!s || !s.messages.length) {
      if (feed) feed.innerHTML = '';
      empty && empty.removeAttribute('hidden');
      if (ctxEl) ctxEl.hidden = true;
      if (title) title.textContent = s ? (s.title || '新对话') : '对话';
      return;
    }
    empty && empty.setAttribute('hidden', '');
    if (title) title.textContent = s.title || '对话';
    if (s.context_text) {
      if (ctxEl) ctxEl.hidden = false;
      if (ctxText) ctxText.textContent = s.context_text;
    } else {
      if (ctxEl) ctxEl.hidden = true;
    }
    if (!feed) return;
    feed.innerHTML = '';
    for (const m of s.messages) feed.appendChild(renderBlock(m));
    requestAnimationFrame(() => { feed.scrollTop = feed.scrollHeight; });
  }

  function renderSessions() {
    const all = loadSessions();
    const cid = getCurrentId();
    const sideList = $('.nav-controls[data-controls="chat"] .nav-sessions');
    if (sideList) {
      const top = all.slice(0, 10);
      sideList.innerHTML = top.length
        ? top.map(s => '<li class="nav-session-item' + (s.id === cid ? ' is-current' : '') + '" data-session-id="' + s.id + '">' +
                       '<span class="nav-session-item-title">' + escapeHtml(s.title) + '</span>' +
                       '<time class="nav-session-item-time">' + fmtRelTime(s.updated_at) + '</time></li>').join('')
        : '<li class="nav-sessions-empty">无</li>';
    }
  }

  function sendMessage(text) {
    const trimmed = (text || '').trim();
    if (!trimmed) return;
    let sid = getCurrentId();
    if (!sid || !findSession(sid)) { newSession(); sid = getCurrentId(); }
    addMessage(sid, 'user', trimmed);
    renderFeed();
    renderSessions();
    setTimeout(() => {
      const ans = pickAnswer(trimmed);
      addMessage(sid, 'agent', ans.text, ans.citations);
      renderFeed();
      renderSessions();
    }, 600);
  }

  const inputField = $('[data-bind="chat-input"]');
  const sendBtn = $('[data-role="chat-send"]');
  function trySend() {
    if (!inputField) return;
    const v = inputField.value || '';
    if (!v.trim()) return;
    sendMessage(v);
    inputField.value = '';
    inputField.style.height = 'auto';
  }
  if (sendBtn) sendBtn.addEventListener('click', trySend);
  if (inputField) {
    inputField.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); trySend(); }
    });
    inputField.addEventListener('input', () => {
      inputField.style.height = 'auto';
      inputField.style.height = Math.min(160, inputField.scrollHeight) + 'px';
    });
  }

  document.addEventListener('click', (e) => {
    const p = e.target.closest('.chat-prompt');
    if (p) sendMessage(p.dataset.prompt);
  });

  // New session + sidebar session list
  document.addEventListener('click', (e) => {
    if (e.target.closest('[data-role="session-new"]')) {
      newSession();
      renderFeed();
      renderSessions();
      requestAnimationFrame(() => inputField && inputField.focus());
      return;
    }
    const item = e.target.closest('.nav-session-item');
    if (item) {
      setCurrentId(item.dataset.sessionId);
      renderFeed();
      renderSessions();
      return;
    }
  });

  const app = document.querySelector('.app');

  // 追问 from story
  document.addEventListener('click', (e) => {
    const btn = e.target.closest('.story-action');
    if (!btn) return;
    const label = btn.querySelector('span') && btn.querySelector('span').textContent.trim();
    if (label !== '追问') return;
    const story = btn.closest('.story');
    if (!story) return;
    e.preventDefault();
    e.stopPropagation();
    const headline = (story.querySelector('.story-headline') || {}).textContent || '';
    const region = story.dataset.region || '';
    newSession({
      title: '追问 · ' + headline.slice(0, 18) + (headline.length > 18 ? '…' : ''),
      context_story_id: region + '-' + Date.now(),
      context_text: headline.trim()
    });
    if (app) {
      app.setAttribute('data-tab', 'chat');
      try { localStorage.setItem('anchormas:tab', 'chat'); } catch (_) {}
    }
    renderFeed();
    renderSessions();
    requestAnimationFrame(() => inputField && inputField.focus());
  }, true);

  // Citation click → source viewer custom event
  document.addEventListener('click', (e) => {
    const citationBtn = e.target.closest('.chat-citation');
    const citeInline  = e.target.closest('.cite[data-ref]');
    if (!citationBtn && !citeInline) return;
    let data;
    if (citationBtn) {
      data = { title: citationBtn.dataset.citeTitle, name: citationBtn.dataset.citeName, time: citationBtn.dataset.citeTime };
    } else {
      const block = citeInline.closest('.chat-block-agent');
      const ref = citeInline.dataset.ref;
      const linked = block && block.querySelector('.chat-citation[data-cite-num="' + ref + '"]');
      if (!linked) return;
      data = { title: linked.dataset.citeTitle, name: linked.dataset.citeName, time: linked.dataset.citeTime };
    }
    document.dispatchEvent(new CustomEvent('anchormas:open-source', { detail: data }));
  });

  renderFeed();
  renderSessions();
})();
