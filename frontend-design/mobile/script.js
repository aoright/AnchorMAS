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
