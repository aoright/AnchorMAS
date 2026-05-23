// Tab 切换（sidebar + tabbar 共享同一 data-tab 状态）
(function () {
  const app = document.querySelector('.app');
  const triggers = document.querySelectorAll('[data-tab]');

  function setTab(tab) {
    if (!tab) return;
    app.setAttribute('data-tab', tab);
    try { localStorage.setItem('anchormas:tab', tab); } catch (_) {}
  }

  triggers.forEach((el) => {
    if (el.tagName === 'BUTTON' || el.tagName === 'A') {
      el.addEventListener('click', () => setTab(el.dataset.tab));
    }
  });

  try {
    const saved = localStorage.getItem('anchormas:tab');
    if (saved && ['brief', 'chat', 'market', 'saved', 'settings'].includes(saved)) setTab(saved);
  } catch (_) {}
})();

/* ============================================================
 * 简报头：date picker + region dropdown
 * 桌面在侧栏 nav-controls，移动端在 brief-meta，两边共享状态
 * ============================================================ */
(function () {
  const $$ = (sel, ctx) => Array.from((ctx || document).querySelectorAll(sel));

  const REGIONS = {
    all: { cn: '全部市场', en: 'All' },
    cn:  { cn: '中国',     en: 'CN'  },
    jp:  { cn: '日本',     en: 'JP'  },
    kr:  { cn: '韩国',     en: 'KR'  },
    sea: { cn: '东南亚',   en: 'SEA' },
    us:  { cn: '美国',     en: 'US'  },
  };
  const DOW = ['Sun','Mon','Tue','Wed','Thu','Fri','Sat'];
  const MON = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];

  function fmtDate(iso) {
    if (!iso) return '';
    const [y, m, d] = iso.split('-').map(Number);
    const dt = new Date(y, m - 1, d);
    return `${DOW[dt.getDay()]} · ${dt.getDate()} ${MON[dt.getMonth()]} ${dt.getFullYear()}`;
  }

  const state = {
    date: '2026-05-23',
    region: 'all',
  };

  try {
    const s = localStorage.getItem('anchormas:brief');
    if (s) Object.assign(state, JSON.parse(s));
  } catch (_) {}

  function persist() {
    try { localStorage.setItem('anchormas:brief', JSON.stringify(state)); } catch (_) {}
  }

  function render() {
    $$('[data-bind="date-text"]').forEach((el) => {
      el.textContent = fmtDate(state.date);
    });
    $$('[data-bind="date-input"]').forEach((el) => {
      el.value = state.date;
    });
    $$('[data-bind="region-text"]').forEach((el) => {
      el.textContent = REGIONS[state.region].cn;
    });
    $$('.region-menu li').forEach((li) => {
      li.setAttribute('aria-selected', li.dataset.region === state.region ? 'true' : 'false');
    });
    // (Filter stories by region — for future; for now just visual state)
  }

  function closeAllMenus() {
    $$('.region-menu').forEach((m) => { m.hidden = true; });
    $$('[data-role="region-trigger"]').forEach((t) => {
      t.setAttribute('aria-expanded', 'false');
    });
  }

  // —— Date trigger
  document.addEventListener('click', (e) => {
    const dateTrig = e.target.closest('[data-role="date-trigger"]');
    if (dateTrig) {
      const input = dateTrig.querySelector('[data-bind="date-input"]');
      if (input) {
        try { input.showPicker && input.showPicker(); } catch (_) { input.click(); input.focus(); }
      }
      e.stopPropagation();
      return;
    }

    const regionTrig = e.target.closest('[data-role="region-trigger"]');
    if (regionTrig) {
      const picker = regionTrig.closest('.region-picker');
      const menu = picker && picker.querySelector('.region-menu');
      if (!menu) return;
      const willOpen = menu.hidden;
      closeAllMenus();
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
      render();
      persist();
      closeAllMenus();
      e.stopPropagation();
      return;
    }

    // outside click → close any open menus
    closeAllMenus();
  });

  document.addEventListener('change', (e) => {
    if (e.target.matches('[data-bind="date-input"]')) {
      state.date = e.target.value;
      render();
      persist();
    }
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') closeAllMenus();
  });

  render();
})();
