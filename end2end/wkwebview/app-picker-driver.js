// Tray "Move to…" in real WKWebView: correlate the maintainer's a11y frame
// (combo box 278x40 at/below the tray) with the DOM, and prove whether the
// top layer paints at all. Screenshots are awaited, not raced.
(() => {
  const log = m => { try { window.webkit.messageHandlers.log.postMessage(String(m)); } catch (e) {} };
  const finish = o => { try { window.webkit.messageHandlers.probe.postMessage(JSON.stringify(o, null, 1)); } catch (e) {} };
  const shot = name => new Promise(res => {
    window.__shotDone = n => { if (n === name) res(); };
    try { window.webkit.messageHandlers.shot.postMessage(name); } catch (e) { res(); }
    setTimeout(res, 5000);
  });
  const sleep = ms => new Promise(r => setTimeout(r, ms));
  const raf = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
  async function until(fn, ms = 25000, step = 100) {
    const t0 = Date.now();
    for (;;) { let v; try { v = fn(); } catch (e) { v = null; }
      if (v) return v; if (Date.now() - t0 > ms) throw new Error('timeout'); await sleep(step); }
  }
  const r = el => { if (!el) return null; const b = el.getBoundingClientRect(); return {
    l: +b.left.toFixed(1), t: +b.top.toFixed(1), r: +b.right.toFixed(1), b: +b.bottom.toFixed(1),
    w: +b.width.toFixed(1), h: +b.height.toFixed(1) }; };

  async function main() {
    const p = location.pathname;
    if (p === '/login') {
      // Credentials are injected by wkprobe.swift from E2E_EMAIL / E2E_PASSWORD
      // (end2end/.env, gitignored) — never hard-coded here.
      await until(() => document.querySelector('input[name=email]'));
      document.querySelector('input[name=email]').value = window.__E2E.email;
      document.querySelector('input[name=password]').value = window.__E2E.password;
      await sleep(150); document.querySelector('button[type=submit]').click(); return;
    }
    await until(() => document.documentElement.getAttribute('data-hydrated') === 'true');
    if (p !== '/my/all') { location.href = '/my/all'; return; }

    const sel = await until(() => document.querySelector('[data-testid="all-cards-row"] [data-testid="row-select"]'));
    sel.click();
    const move = await until(() => document.querySelector('[data-testid="tray-move"]'));
    await sleep(800);
    await shot('tray-1-docked');

    move.click();
    await raf(); await sleep(1800); await raf();

    const panel = document.getElementById('popover-tray-destination');
    const cmd = panel.querySelector('[data-name="Command"]');
    const input = panel.querySelector('input');
    const list = panel.querySelector('[data-name="CommandList"]');
    const tray = document.querySelector('[data-testid="selection-tray"]');
    const out = {
      viewport: { w: innerWidth, h: innerHeight },
      open: panel.matches(':popover-open'),
      trigger: r(move), tray: r(tray), panel: r(panel),
      command: r(cmd), commandInput: r(input), commandList: r(list),
      commandComputed: cmd ? { height: getComputedStyle(cmd).height, overflow: getComputedStyle(cmd).overflow,
        display: getComputedStyle(cmd).display } : null,
      // Does the top layer actually PAINT above the tray? Ask the engine what it
      // hit-tests at the panel's own centre — if the popover is on top, that
      // point belongs to the panel's subtree, not the tray's.
      hitTestAtPanelCentre: (() => {
        const b = panel.getBoundingClientRect();
        const el = document.elementFromPoint((b.left + b.right) / 2, (b.top + b.bottom) / 2);
        return el ? { tag: el.tagName.toLowerCase(),
          name: el.getAttribute('data-name') || el.getAttribute('data-testid') || null,
          insidePanel: panel.contains(el), insideTray: tray ? tray.contains(el) : null } : null;
      })(),
      // Same question one row down into where the CommandInput's layout box sits
      // (the frame the maintainer's accessibility read reported).
      hitTestAtInputCentre: (() => {
        if (!input) return null;
        const b = input.getBoundingClientRect();
        const el = document.elementFromPoint((b.left + b.right) / 2, (b.top + b.bottom) / 2);
        return el ? { tag: el.tagName.toLowerCase(),
          name: el.getAttribute('data-name') || el.getAttribute('data-testid') || null,
          insidePanel: panel.contains(el), insideTray: tray ? tray.contains(el) : null } : null;
      })(),
    };
    await shot('tray-2-move-open');
    finish(out);
  }
  main().catch(e => finish({ driverError: String(e && e.stack || e), at: location.pathname }));
})();
