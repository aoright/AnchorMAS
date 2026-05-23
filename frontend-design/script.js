// Workbench: 切换 双视图 / 桌面 / 手机
(function () {
  const stage = document.querySelector('.stage');
  const buttons = document.querySelectorAll('.view-btn');

  function setView(view) {
    stage.setAttribute('data-view', view);
    buttons.forEach((b) => {
      const active = b.dataset.view === view;
      b.classList.toggle('active', active);
      b.setAttribute('aria-selected', String(active));
    });
    try { localStorage.setItem('anchormas:view', view); } catch (_) {}
  }

  buttons.forEach((b) => {
    b.addEventListener('click', () => setView(b.dataset.view));
  });

  // 恢复上次选择
  try {
    const saved = localStorage.getItem('anchormas:view');
    if (saved && ['split', 'desktop', 'mobile'].includes(saved)) setView(saved);
  } catch (_) {}
})();
