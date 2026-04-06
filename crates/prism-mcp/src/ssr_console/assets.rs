pub(crate) const HTMX_CDN: &str = "https://unpkg.com/htmx.org@2.0.4";
pub(crate) const MERMAID_CDN: &str = "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js";
pub(crate) const VIS_TIMELINE_JS_CDN: &str =
    "https://unpkg.com/vis-timeline@7.7.4/standalone/umd/vis-timeline-graph2d.min.js";
pub(crate) const VIS_TIMELINE_CSS_CDN: &str =
    "https://unpkg.com/vis-timeline@7.7.4/styles/vis-timeline-graph2d.min.css";

pub(crate) fn console_css() -> &'static str {
    r#"
:root {
  --console-bg: #f4efe2;
  --console-ink: #10211a;
  --console-muted: #5e6b63;
  --console-border: rgba(16, 33, 26, 0.18);
  --console-panel: rgba(255, 252, 245, 0.88);
  --console-panel-strong: #fffaf1;
  --console-accent: #1f5f4a;
  --console-accent-soft: #d7efe5;
  --console-warn: #b4542c;
  --console-done: #25684d;
  --console-active: #0b5a78;
  --console-pending: #7a847f;
  --console-shadow: 0 20px 60px rgba(16, 33, 26, 0.12);
}

* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
body {
  font-family: "Avenir Next", "Segoe UI", sans-serif;
  color: var(--console-ink);
  background:
    radial-gradient(circle at top left, rgba(31, 95, 74, 0.18), transparent 28%),
    radial-gradient(circle at bottom right, rgba(180, 84, 44, 0.12), transparent 32%),
    linear-gradient(180deg, #f7f1e3, #efe8d8 68%, #ece4d3);
}
a { color: var(--console-accent); text-decoration: none; }
a:hover { text-decoration: underline; }
code, pre, textarea, input, select, button { font-family: "SF Mono", "Menlo", monospace; }

.console-shell {
  min-height: 100vh;
  padding: 24px;
}
.console-frame {
  max-width: 1540px;
  margin: 0 auto;
  display: grid;
  gap: 18px;
}
.console-topbar,
.console-panel,
.console-sidebar-card,
.console-card,
.console-task-card,
.console-doc,
.console-kpi {
  backdrop-filter: blur(10px);
  background: var(--console-panel);
  border: 1px solid var(--console-border);
  box-shadow: var(--console-shadow);
}
.console-topbar {
  border-radius: 24px;
  padding: 18px 22px;
  display: grid;
  gap: 14px;
}
.console-brand {
  display: flex;
  justify-content: space-between;
  align-items: start;
  gap: 16px;
}
.console-brand h1 {
  margin: 0;
  font-size: clamp(1.9rem, 3vw, 3rem);
  line-height: 0.95;
  letter-spacing: -0.04em;
}
.console-eyebrow {
  margin: 0 0 6px;
  color: var(--console-muted);
  font-size: 0.78rem;
  font-weight: 700;
  letter-spacing: 0.18em;
  text-transform: uppercase;
}
.console-subtitle {
  margin: 0;
  max-width: 72ch;
  color: var(--console-muted);
  font-size: 0.98rem;
}
.console-nav {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.console-nav a {
  padding: 10px 14px;
  border-radius: 999px;
  border: 1px solid var(--console-border);
  background: rgba(255,255,255,0.55);
  font-size: 0.92rem;
}
.console-nav a[data-active="true"] {
  background: var(--console-ink);
  color: white;
  border-color: var(--console-ink);
}
.console-meta-grid,
.console-kpi-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 12px;
}
.console-kpi, .console-meta-card {
  border-radius: 18px;
  padding: 14px 16px;
}
.console-kpi strong,
.console-meta-card strong {
  display: block;
  font-size: 1.3rem;
  margin-top: 6px;
}
.console-layout {
  display: grid;
  gap: 18px;
}
.console-layout.console-layout--two {
  grid-template-columns: minmax(290px, 360px) minmax(0, 1fr);
  align-items: start;
}
.console-layout.console-layout--two > *,
.console-grid-two > *,
.console-sidebar,
.console-main,
.console-card,
.console-task-card,
.console-sidebar-card,
.console-doc {
  min-width: 0;
}
.console-layout.console-layout--single {
  grid-template-columns: minmax(0, 1fr);
}
.console-sidebar,
.console-main {
  display: grid;
  gap: 18px;
}
.console-sidebar-card,
.console-card,
.console-task-card,
.console-doc {
  border-radius: 24px;
  padding: 18px 20px;
}
.console-card-header {
  display: flex;
  justify-content: space-between;
  align-items: start;
  gap: 12px;
  margin-bottom: 12px;
}
.console-card-header h2,
.console-card-header h3,
.console-card h2,
.console-card h3,
.console-task-card h2 {
  margin: 0;
  letter-spacing: -0.03em;
}
.console-list,
.console-inline-list {
  list-style: none;
  padding: 0;
  margin: 0;
}
.console-list {
  display: grid;
  gap: 10px;
}
.console-plan-link,
.console-task-link,
.console-concept-link {
  display: grid;
  gap: 8px;
  padding: 14px;
  border-radius: 18px;
  border: 1px solid var(--console-border);
  background: rgba(255,255,255,0.62);
  overflow: hidden;
}
.console-plan-link strong,
.console-task-link strong,
.console-concept-link strong,
.console-card-header strong,
.console-card-header h2,
.console-card-header h3,
.console-card h2,
.console-card h3,
.console-task-card h2 {
  overflow-wrap: anywhere;
}
.console-plan-link[data-selected="true"] {
  border-color: var(--console-accent);
  background: var(--console-accent-soft);
}
.console-inline-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.console-pill,
.console-status {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border-radius: 999px;
  padding: 6px 10px;
  font-size: 0.78rem;
  font-weight: 700;
  letter-spacing: 0.03em;
}
.console-status--active,
.console-status--inprogress,
.console-status--leased { background: rgba(11, 90, 120, 0.12); color: var(--console-active); }
.console-status--done,
.console-status--completed { background: rgba(37, 104, 77, 0.12); color: var(--console-done); }
.console-status--pending,
.console-status--ready,
.console-status--proposed,
.console-status--draft { background: rgba(122, 132, 127, 0.14); color: var(--console-pending); }
.console-status--blocked,
.console-status--abandoned,
.console-status--archived { background: rgba(180, 84, 44, 0.14); color: var(--console-warn); }
.console-progress {
  width: 100%;
  height: 9px;
  border-radius: 999px;
  background: rgba(16, 33, 26, 0.1);
  overflow: hidden;
}
.console-progress > span {
  display: block;
  height: 100%;
  background: linear-gradient(90deg, var(--console-accent), #2f8d6d);
}
.console-filter-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  gap: 10px;
}
.console-field {
  display: grid;
  gap: 6px;
}
.console-field label {
  font-size: 0.8rem;
  color: var(--console-muted);
  text-transform: uppercase;
  letter-spacing: 0.08em;
}
.console-input,
.console-select,
.console-textarea {
  width: 100%;
  border-radius: 14px;
  border: 1px solid rgba(16, 33, 26, 0.16);
  background: rgba(255,255,255,0.8);
  padding: 10px 12px;
  color: var(--console-ink);
}
.console-textarea { min-height: 120px; resize: vertical; }
.console-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}
.console-copy-action {
  display: inline-flex;
  align-items: center;
  gap: 10px;
}
.console-action-form {
  display: inline-flex;
}
.console-action-form.htmx-request .console-button {
  pointer-events: none;
  opacity: 0.86;
}
.console-button[disabled],
.console-button.is-busy {
  pointer-events: none;
  opacity: 0.86;
}
.console-action-label {
  display: inline-flex;
}
.console-action-spinner {
  width: 0.95rem;
  height: 0.95rem;
  border-radius: 999px;
  border: 2px solid currentColor;
  border-right-color: transparent;
  display: none;
  animation: console-spin 700ms linear infinite;
}
.console-action-form.htmx-request .console-action-spinner {
  display: inline-block;
}
.console-button.is-busy .console-action-spinner {
  display: inline-block;
}
.console-action-form.htmx-request .console-action-label {
  opacity: 0.82;
}
.console-button.is-busy .console-action-label {
  opacity: 0.82;
}
.console-action-feedback {
  min-height: 1rem;
  color: var(--console-muted);
}
.console-action-feedback[data-state="success"] {
  color: var(--console-done);
}
.console-action-feedback[data-state="error"] {
  color: var(--console-warn);
}
.console-button {
  border: 0;
  border-radius: 999px;
  padding: 10px 14px;
  background: var(--console-ink);
  color: white;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}
.console-button--ghost {
  background: rgba(255,255,255,0.6);
  color: var(--console-ink);
  border: 1px solid var(--console-border);
}
.console-button--small {
  padding: 8px 12px;
  font-size: 0.82rem;
}
.console-button--warn {
  background: var(--console-warn);
}
.console-doc {
  line-height: 1.6;
}
.console-doc h1,
.console-doc h2,
.console-doc h3 { letter-spacing: -0.03em; }
.console-doc pre {
  overflow: auto;
  border-radius: 18px;
  padding: 14px;
  background: #10211a;
  color: #f8f3e8;
}
.console-doc code {
  background: rgba(16, 33, 26, 0.08);
  padding: 0.08rem 0.3rem;
  border-radius: 6px;
}
.console-grid-two {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 18px;
}
.console-data-table {
  width: 100%;
  border-collapse: collapse;
}
.console-data-table th,
.console-data-table td {
  padding: 10px 8px;
  border-bottom: 1px solid rgba(16, 33, 26, 0.08);
  vertical-align: top;
  text-align: left;
}
.console-data-table th {
  font-size: 0.8rem;
  color: var(--console-muted);
  text-transform: uppercase;
  letter-spacing: 0.08em;
}
.console-mermaid {
  padding: 12px;
  overflow: auto;
  border-radius: 18px;
  background: rgba(255,255,255,0.56);
}
.console-mermaid svg {
  display: block;
  max-width: 100%;
  height: auto;
}
.console-graph-card {
  overflow: hidden;
}
.console-graph-shell {
  position: relative;
  border: 1px solid rgba(16, 33, 26, 0.12);
  border-radius: 20px;
  background: rgba(255,255,255,0.58);
}
.console-graph-controls {
  display: flex;
  justify-content: flex-end;
  flex-wrap: wrap;
  gap: 8px;
  padding: 12px 12px 0;
}
.console-graph-viewport {
  min-height: 620px;
  margin: 12px;
  overflow: hidden;
  border-radius: 18px;
  background: rgba(255,255,255,0.78);
  cursor: grab;
  touch-action: none;
}
.console-graph-viewport.is-dragging {
  cursor: grabbing;
}
.console-graph-viewport .console-mermaid {
  min-height: 100%;
  margin: 0;
  padding: 24px;
  overflow: visible;
  background: transparent;
  display: flex;
  justify-content: center;
  align-items: center;
}
.console-graph-viewport .console-mermaid svg {
  max-width: none;
  transform-origin: center center;
  will-change: transform;
}
.console-graph-shell.is-fullscreen {
  position: fixed;
  inset: 18px;
  z-index: 1000;
  background: rgba(255, 250, 241, 0.98);
  box-shadow: 0 28px 90px rgba(16, 33, 26, 0.28);
}
.console-graph-shell.is-fullscreen .console-graph-viewport {
  min-height: calc(100vh - 120px);
}
body.console-graph-fullscreen-open {
  overflow: hidden;
}
.console-fleet-host {
  min-height: 420px;
  border-radius: 18px;
  overflow: hidden;
  border: 1px solid rgba(16, 33, 26, 0.12);
  background: rgba(255,255,255,0.6);
}
.console-fleet-host .vis-item {
  border-width: 1px;
  border-radius: 999px;
  font-size: 0.78rem;
  box-shadow: none;
}
.console-fleet-host .vis-item.fleet-active {
  background: rgba(11, 90, 120, 0.16);
  border-color: rgba(11, 90, 120, 0.5);
  color: var(--console-ink);
}
.console-fleet-host .vis-item.fleet-stale {
  background: rgba(180, 84, 44, 0.18);
  border-color: rgba(180, 84, 44, 0.55);
  color: var(--console-ink);
}
.console-fleet-host .vis-item.fleet-complete {
  background: rgba(37, 104, 77, 0.12);
  border-color: rgba(37, 104, 77, 0.35);
  color: var(--console-ink);
}
.console-fleet-host .vis-time-axis .vis-text {
  color: var(--console-muted);
}
.console-fleet-host .vis-labelset .vis-label {
  background: rgba(255,255,255,0.8);
  color: var(--console-ink);
  border-color: rgba(16, 33, 26, 0.08);
}
.console-muted { color: var(--console-muted); }
.console-small { font-size: 0.88rem; }
.console-stack { display: grid; gap: 14px; }
.console-empty {
  padding: 28px;
  border-radius: 18px;
  border: 1px dashed rgba(16, 33, 26, 0.24);
  color: var(--console-muted);
  background: rgba(255,255,255,0.4);
}
.console-live {
  position: relative;
}
.console-sync {
  opacity: 0;
  transition: opacity 120ms ease;
  color: var(--console-muted);
  font-size: 0.82rem;
}
.htmx-request .console-sync,
.htmx-request.console-sync {
  opacity: 1;
}
.console-live.htmx-request {
  opacity: 0.72;
}
@keyframes console-spin {
  to { transform: rotate(360deg); }
}

@media (max-width: 1040px) {
  .console-layout.console-layout--two,
  .console-grid-two {
    grid-template-columns: minmax(0, 1fr);
  }
  .console-graph-viewport {
    min-height: 440px;
  }
  .console-shell { padding: 14px; }
}
"#
}

pub(crate) fn console_js() -> &'static str {
    r#"
function prismConsoleClamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function prismConsoleInitInteractiveGraphs(root = document) {
  const hosts = root.querySelectorAll('[data-console-graph]:not([data-graph-bound="true"])');
  for (const host of hosts) {
    const viewport = host.querySelector('[data-graph-viewport]');
    const diagram = host.querySelector('.prism-mermaid');
    const svg = diagram && diagram.querySelector('svg');
    if (!viewport || !diagram || !svg) continue;
    host.dataset.graphBound = 'true';
    svg.querySelectorAll('a').forEach((link) => {
      const href = link.getAttribute('href') || link.getAttribute('xlink:href') || link.href?.baseVal || null;
      link.setAttribute('target', '_self');
      if (href) {
        link.dataset.prismGraphHref = href;
        link.querySelectorAll('*').forEach((node) => {
          if (node instanceof Element) {
            node.dataset.prismGraphHref = href;
          }
        });
      }
      if (link.dataset.prismGraphLinkBound === 'true') return;
      link.dataset.prismGraphLinkBound = 'true';
      link.addEventListener('click', (event) => {
        const targetHref = link.dataset.prismGraphHref || link.getAttribute('href') || link.getAttribute('xlink:href') || link.href?.baseVal;
        if (!targetHref) return;
        event.preventDefault();
        event.stopPropagation();
        window.location.assign(targetHref);
      });
    });
    const state = { scale: 1, x: 0, y: 0, pointerId: null, lastX: 0, lastY: 0, dragged: false };
    const zoomStep = 1.024;
    host.__prismGraphState = state;
    const fullscreenButton = host.querySelector('[data-graph-fullscreen]');
    const graphHrefForEvent = (event) => {
      if (!event) return null;
      const path = typeof event.composedPath === 'function' ? event.composedPath() : [];
      for (const entry of path) {
        if (!(entry instanceof Element)) continue;
        const href = entry.dataset?.prismGraphHref
          || entry.getAttribute?.('href')
          || entry.getAttribute?.('xlink:href')
          || entry.href?.baseVal
          || null;
        if (href) return href;
      }
      if (Number.isFinite(event.clientX) && Number.isFinite(event.clientY)) {
        const hits = document.elementsFromPoint(event.clientX, event.clientY);
        for (const hit of hits) {
          if (!(hit instanceof Element)) continue;
          const href = hit.dataset?.prismGraphHref
            || hit.getAttribute?.('href')
            || hit.getAttribute?.('xlink:href')
            || hit.href?.baseVal
            || hit.closest?.('[data-prism-graph-href]')?.dataset?.prismGraphHref
            || hit.closest?.('a')?.getAttribute?.('href')
            || hit.closest?.('a')?.getAttribute?.('xlink:href')
            || hit.closest?.('a')?.href?.baseVal
            || null;
          if (href) return href;
        }
      }
      const target = event.target;
      if (!(target instanceof Element)) return null;
      return target.dataset?.prismGraphHref
        || target.closest?.('[data-prism-graph-href]')?.dataset?.prismGraphHref
        || target.closest?.('a')?.getAttribute?.('href')
        || target.closest?.('a')?.getAttribute?.('xlink:href')
        || target.closest?.('a')?.href?.baseVal
        || null;
    };
    const applyTransform = () => {
      svg.style.transform = `translate(${state.x}px, ${state.y}px) scale(${state.scale})`;
    };
    const setFullscreen = (enabled) => {
      host.classList.toggle('is-fullscreen', enabled);
      document.body.classList.toggle('console-graph-fullscreen-open', enabled);
      if (fullscreenButton) {
        fullscreenButton.textContent = enabled ? 'Exit full page' : 'Full page';
      }
      window.__prismActiveFullscreenGraph = enabled ? host : null;
    };
    const reset = () => {
      state.scale = 1;
      state.x = 0;
      state.y = 0;
      applyTransform();
    };
    const zoom = (factor) => {
      state.scale = prismConsoleClamp(state.scale * factor, 0.45, 3.5);
      applyTransform();
    };
    viewport.addEventListener('wheel', (event) => {
      event.preventDefault();
      zoom(event.deltaY < 0 ? zoomStep : 1 / zoomStep);
    }, { passive: false });
    viewport.addEventListener('click', (event) => {
      if (state.dragged) return;
      const href = graphHrefForEvent(event);
      if (!href) return;
      event.preventDefault();
      event.stopPropagation();
      window.location.assign(href);
    }, true);
    viewport.addEventListener('pointerdown', (event) => {
      if (event.button !== 0) return;
      state.pointerId = event.pointerId;
      state.lastX = event.clientX;
      state.lastY = event.clientY;
      state.dragged = false;
      state.dragDistance = 0;
      viewport.classList.add('is-dragging');
      viewport.setPointerCapture(event.pointerId);
    });
    viewport.addEventListener('pointermove', (event) => {
      if (state.pointerId !== event.pointerId) return;
      const dx = event.clientX - state.lastX;
      const dy = event.clientY - state.lastY;
      state.dragDistance += Math.abs(dx) + Math.abs(dy);
      if (state.dragDistance > 4) {
        state.dragged = true;
        state.x += dx;
        state.y += dy;
        applyTransform();
      }
      state.lastX = event.clientX;
      state.lastY = event.clientY;
    });
    const endDrag = (event) => {
      if (state.pointerId !== event.pointerId) return;
      state.pointerId = null;
      viewport.classList.remove('is-dragging');
      if (viewport.hasPointerCapture(event.pointerId)) {
        viewport.releasePointerCapture(event.pointerId);
      }
      if (state.dragged) {
        const suppressClick = (clickEvent) => {
          clickEvent.preventDefault();
          clickEvent.stopPropagation();
          viewport.removeEventListener('click', suppressClick, true);
        };
        viewport.addEventListener('click', suppressClick, true);
      }
    };
    viewport.addEventListener('pointerup', endDrag);
    viewport.addEventListener('pointercancel', endDrag);
    host.querySelector('[data-graph-zoom-in]')?.addEventListener('click', () => zoom(zoomStep));
    host.querySelector('[data-graph-zoom-out]')?.addEventListener('click', () => zoom(1 / zoomStep));
    host.querySelector('[data-graph-reset]')?.addEventListener('click', reset);
    fullscreenButton?.addEventListener('click', () => {
      setFullscreen(!host.classList.contains('is-fullscreen'));
    });
    applyTransform();
  }
}

function prismConsoleInitMermaid(root = document) {
  if (!window.mermaid) return;
  if (!window.__prismMermaidInitialized) {
    window.mermaid.initialize({
      startOnLoad: false,
      theme: 'neutral',
      flowchart: { defaultRenderer: 'elk', curve: 'basis' },
      securityLevel: 'loose'
    });
    window.__prismMermaidInitialized = true;
  }
  const nodes = root.querySelectorAll('.prism-mermaid:not([data-mermaid-bound="true"])');
  if (nodes.length === 0) {
    prismConsoleInitInteractiveGraphs(root);
    return;
  }
  for (const node of nodes) node.dataset.mermaidBound = 'true';
  Promise.resolve(window.mermaid.run({ nodes })).finally(() => {
    prismConsoleInitInteractiveGraphs(root);
  });
}

function prismConsoleInitTimeline(root = document) {
  const hosts = root.querySelectorAll('[data-prism-fleet-host]');
  if (!window.vis || hosts.length === 0) return;
  for (const host of hosts) {
    const payloadEl = host.querySelector('script[type="application/json"]');
    if (!payloadEl) continue;
    let payload;
    try {
      payload = JSON.parse(payloadEl.textContent || '{}');
    } catch (_error) {
      continue;
    }
    if (host.__prismTimeline) {
      host.__prismTimeline.destroy();
      host.__prismTimeline = null;
    }
    const groups = new window.vis.DataSet(payload.groups || []);
    const items = new window.vis.DataSet(payload.items || []);
    const timeline = new window.vis.Timeline(host, items, groups, {
      stack: false,
      orientation: 'top',
      zoomKey: 'ctrlKey',
      selectable: true,
      margin: { item: 12, axis: 10 },
      showCurrentTime: true,
      horizontalScroll: true,
      zoomMin: 1000 * 60 * 10,
      tooltip: { followMouse: true, overflowMethod: 'cap' }
    });
    timeline.on('select', (properties) => {
      const selectedId = properties.items && properties.items[0];
      if (!selectedId) return;
      const item = items.get(selectedId);
      if (item && item.taskUrl) {
        window.location.assign(item.taskUrl);
      }
    });
    host.__prismTimeline = timeline;
  }
}

async function prismConsoleCopyText(text) {
  if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
    await navigator.clipboard.writeText(text);
    return;
  }
  const field = document.createElement('textarea');
  field.value = text;
  field.setAttribute('readonly', 'readonly');
  field.style.position = 'fixed';
  field.style.opacity = '0';
  document.body.appendChild(field);
  field.select();
  document.execCommand('copy');
  document.body.removeChild(field);
}

function prismConsoleInitPlanMarkdownActions(root = document) {
  const buttons = root.querySelectorAll('[data-copy-markdown-url]:not([data-copy-bound="true"])');
  for (const button of buttons) {
    button.dataset.copyBound = 'true';
    const feedback = button.parentElement?.querySelector('[data-copy-markdown-feedback]') || null;
    let resetTimer = null;
    const setFeedback = (state, message) => {
      if (!feedback) return;
      feedback.dataset.state = state || '';
      feedback.textContent = message || '';
    };
    button.addEventListener('click', async () => {
      const url = button.dataset.copyMarkdownUrl;
      if (!url) return;
      if (resetTimer) {
        window.clearTimeout(resetTimer);
        resetTimer = null;
      }
      button.classList.add('is-busy');
      button.setAttribute('disabled', 'disabled');
      setFeedback('', 'Copying…');
      try {
        const response = await fetch(url, { headers: { 'X-Requested-With': 'fetch' } });
        if (!response.ok) {
          throw new Error(`copy failed with status ${response.status}`);
        }
        await prismConsoleCopyText(await response.text());
        setFeedback('success', 'Copied');
      } catch (_error) {
        setFeedback('error', 'Copy failed');
      } finally {
        button.classList.remove('is-busy');
        button.removeAttribute('disabled');
        resetTimer = window.setTimeout(() => setFeedback('', ''), 1600);
      }
    });
  }
}

function prismConsoleBoot(root = document) {
  prismConsoleInitMermaid(root);
  prismConsoleInitTimeline(root);
  prismConsoleInitPlanMarkdownActions(root);
}

document.addEventListener('DOMContentLoaded', () => prismConsoleBoot(document));
document.addEventListener('htmx:afterSwap', (event) => {
  prismConsoleBoot(event.target || document);
});
document.addEventListener('keydown', (event) => {
  if (event.key !== 'Escape') return;
  const host = window.__prismActiveFullscreenGraph;
  if (!host) return;
  host.classList.remove('is-fullscreen');
  document.body.classList.remove('console-graph-fullscreen-open');
  const button = host.querySelector('[data-graph-fullscreen]');
  if (button) button.textContent = 'Full page';
  window.__prismActiveFullscreenGraph = null;
});
"#
}
