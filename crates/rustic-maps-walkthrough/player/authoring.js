'use strict';

/*
 * Development-only hotspot authoring overlay.
 *
 * The local server injects this script when it serves a built directory; the
 * production player never contains it. Press E to show or hide the panel, drag
 * on the phone frame to draw a zone, then copy the generated hotspot JSON.
 */
(function () {
  const dataElement = document.getElementById('walkthrough-data');
  const phone = document.getElementById('phone');
  if (!dataElement || !phone) {
    return;
  }

  const data = JSON.parse(dataElement.textContent);
  const platforms = data.platforms || {};
  const platformOrder = (data.platform_order || Object.keys(platforms)).filter(
    (key) => platforms[key] && Array.isArray(platforms[key].screens)
  );
  if (platformOrder.length === 0) {
    return;
  }

  const PERCENT_MAXIMUM = 100;
  const MINIMUM_ZONE_PERCENT = 1;
  const DECIMAL_PLACES = 2;
  const TOGGLE_KEY = 'e';
  const SLUG_FALLBACK = 'hotspot';
  const HASH_SEPARATOR = '/';

  const stylesheet = document.createElement('style');
  stylesheet.textContent = `
    .walkthrough-authoring {
      position: fixed;
      right: 16px;
      bottom: 16px;
      z-index: 50;
      width: min(320px, calc(100vw - 32px));
      padding: 16px;
      border: 1px solid var(--border, #d4d4d8);
      border-radius: 14px;
      background: var(--surface, #ffffff);
      color: var(--foreground, #18181b);
      box-shadow: 0 16px 40px rgb(0 0 0 / 24%);
      font: 14px/1.4 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
    }
    .walkthrough-authoring[hidden] { display: none; }
    .walkthrough-authoring h2 { margin: 0 0 6px; font-size: 15px; }
    .walkthrough-authoring p { margin: 0 0 10px; color: var(--muted, #52525b); }
    .walkthrough-authoring label { display: block; margin: 0 0 8px; font-weight: 600; }
    .walkthrough-authoring input,
    .walkthrough-authoring select {
      display: block;
      width: 100%;
      margin-top: 4px;
      padding: 6px 8px;
      border: 1px solid var(--border, #d4d4d8);
      border-radius: 8px;
      background: var(--background, #f4f4f5);
      color: inherit;
      font: inherit;
    }
    .walkthrough-authoring output {
      display: block;
      margin: 8px 0;
      font-variant-numeric: tabular-nums;
    }
    .walkthrough-authoring button {
      width: 100%;
      min-height: 38px;
      border: 1px solid var(--accent, #2563eb);
      border-radius: 999px;
      background: var(--accent, #2563eb);
      color: var(--accent-contrast, #ffffff);
      font: inherit;
      cursor: pointer;
    }
    .walkthrough-authoring button:disabled { opacity: 0.45; cursor: not-allowed; }
    .walkthrough-authoring [role="status"] { min-height: 1.4em; margin: 8px 0 0; }
    .authoring-zone {
      position: absolute;
      z-index: 10;
      display: none;
      border: 2px dashed #f59e0b;
      background: rgb(245 158 11 / 22%);
      pointer-events: none;
    }
  `;
  document.head.append(stylesheet);

  const panel = document.createElement('aside');
  panel.className = 'walkthrough-authoring';
  panel.setAttribute('aria-label', 'Hotspot authoring');
  panel.innerHTML = `
    <h2>Hotspot authoring</h2>
    <p>Drag on the phone frame to draw a zone. Press E to hide this panel.</p>
    <label>Label<input class="authoring-label" type="text" placeholder="Layers control"></label>
    <label>Gesture
      <select class="authoring-gesture">
        <option value="tap">tap</option>
        <option value="swipe-up">swipe-up</option>
        <option value="swipe-down">swipe-down</option>
        <option value="swipe-left">swipe-left</option>
        <option value="swipe-right">swipe-right</option>
      </select>
    </label>
    <label>Next screen<select class="authoring-next-screen"></select></label>
    <output class="authoring-rect">No zone drawn</output>
    <button class="authoring-copy" type="button" disabled>Copy hotspot JSON</button>
    <p role="status" class="authoring-status"></p>
  `;
  document.body.append(panel);

  const labelInput = panel.querySelector('.authoring-label');
  const gestureSelect = panel.querySelector('.authoring-gesture');
  const nextScreenSelect = panel.querySelector('.authoring-next-screen');
  const rectOutput = panel.querySelector('.authoring-rect');
  const copyButton = panel.querySelector('.authoring-copy');
  const statusOutput = panel.querySelector('.authoring-status');

  const zone = document.createElement('div');
  zone.className = 'authoring-zone';
  phone.append(zone);

  let isVisible = true;
  let isDrawing = false;
  let startPoint = null;
  let currentRect = null;

  function currentPlatformKey() {
    const raw = window.location.hash.replace(/^#/, '');
    const key = raw.split(HASH_SEPARATOR)[0];
    return platforms[key] ? key : platformOrder[0];
  }

  function refreshScreenOptions() {
    const platform = platforms[currentPlatformKey()];
    const selected = nextScreenSelect.value;
    nextScreenSelect.replaceChildren();
    for (const screen of platform.screens) {
      const option = document.createElement('option');
      option.value = screen.id;
      option.textContent = screen.id;
      nextScreenSelect.append(option);
    }
    if (selected) {
      nextScreenSelect.value = selected;
    }
  }

  function setVisibility(visible) {
    isVisible = visible;
    panel.hidden = !visible;
    document.body.classList.toggle('authoring-active', visible);
    if (visible) {
      refreshScreenOptions();
    } else {
      zone.style.display = 'none';
      isDrawing = false;
      startPoint = null;
    }
  }

  function frameMetrics() {
    const bounds = phone.getBoundingClientRect();
    return {
      left: bounds.left + phone.clientLeft,
      top: bounds.top + phone.clientTop,
      width: phone.clientWidth,
      height: phone.clientHeight
    };
  }

  function percentRect(start, end) {
    const metrics = frameMetrics();
    if (metrics.width === 0 || metrics.height === 0) {
      return null;
    }
    const startX = ((Math.min(start.x, end.x) - metrics.left) / metrics.width) * PERCENT_MAXIMUM;
    const startY = ((Math.min(start.y, end.y) - metrics.top) / metrics.height) * PERCENT_MAXIMUM;
    const endX = ((Math.max(start.x, end.x) - metrics.left) / metrics.width) * PERCENT_MAXIMUM;
    const endY = ((Math.max(start.y, end.y) - metrics.top) / metrics.height) * PERCENT_MAXIMUM;
    const clamp = (value) => Math.min(PERCENT_MAXIMUM, Math.max(0, value));
    const left = clamp(startX);
    const top = clamp(startY);
    const right = clamp(endX);
    const bottom = clamp(endY);
    return {
      top: round(top),
      left: round(left),
      width: round(right - left),
      height: round(bottom - top)
    };
  }

  function round(value) {
    const factor = 10 ** DECIMAL_PLACES;
    return Math.round(value * factor) / factor;
  }

  function drawZone(rect) {
    if (!rect) {
      zone.style.display = 'none';
      return;
    }
    zone.style.display = 'block';
    zone.style.top = `${rect.top}%`;
    zone.style.left = `${rect.left}%`;
    zone.style.width = `${rect.width}%`;
    zone.style.height = `${rect.height}%`;
  }

  function isFormTarget(target) {
    return (
      target &&
      (target.isContentEditable ||
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.tagName === 'SELECT')
    );
  }

  function slug(label) {
    const normalized = label
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '');
    return normalized || SLUG_FALLBACK;
  }

  function hotspotJson() {
    if (!currentRect) {
      return null;
    }
    return JSON.stringify(
      {
        id: slug(labelInput.value.trim()),
        label: labelInput.value.trim(),
        gesture: gestureSelect.value,
        rect: currentRect,
        next_screen: nextScreenSelect.value
      },
      null,
      2
    );
  }

  async function copyJson() {
    const json = hotspotJson();
    if (!json) {
      return;
    }
    try {
      await navigator.clipboard.writeText(json);
      statusOutput.textContent = 'Copied hotspot JSON.';
    } catch (error) {
      const fallback = document.createElement('textarea');
      fallback.value = json;
      fallback.setAttribute('readonly', '');
      fallback.style.position = 'fixed';
      fallback.style.opacity = '0';
      document.body.append(fallback);
      fallback.select();
      const copied = document.execCommand('copy');
      fallback.remove();
      statusOutput.textContent = copied
        ? 'Copied hotspot JSON.'
        : 'Copy failed; select the JSON from the console instead.';
      if (!copied) {
        console.info(json);
      }
    }
  }

  phone.addEventListener(
    'pointerdown',
    (event) => {
      if (!isVisible || event.button !== 0) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      isDrawing = true;
      startPoint = { x: event.clientX, y: event.clientY };
      currentRect = null;
      copyButton.disabled = true;
      rectOutput.textContent = 'Drawing...';
      drawZone(null);
      refreshScreenOptions();
      try {
        phone.setPointerCapture(event.pointerId);
      } catch (error) {
        // Pointer capture is an enhancement; the phone still receives moves.
      }
    },
    true
  );

  phone.addEventListener(
    'pointermove',
    (event) => {
      if (!isVisible || !isDrawing || !startPoint) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      const candidate = percentRect(startPoint, {
        x: event.clientX,
        y: event.clientY
      });
      if (candidate) {
        drawZone(candidate);
        rectOutput.textContent = `top ${candidate.top}% · left ${candidate.left}% · width ${candidate.width}% · height ${candidate.height}%`;
      }
    },
    true
  );

  function finishDrawing(event) {
    if (!isVisible || !isDrawing || !startPoint) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    const candidate = percentRect(startPoint, {
      x: event.clientX,
      y: event.clientY
    });
    isDrawing = false;
    startPoint = null;
    if (
      candidate &&
      candidate.width >= MINIMUM_ZONE_PERCENT &&
      candidate.height >= MINIMUM_ZONE_PERCENT
    ) {
      currentRect = candidate;
      drawZone(candidate);
      copyButton.disabled = false;
      statusOutput.textContent = '';
    } else {
      currentRect = null;
      drawZone(null);
      rectOutput.textContent = 'No zone drawn';
      copyButton.disabled = true;
    }
  }

  phone.addEventListener('pointerup', finishDrawing, true);
  phone.addEventListener('pointercancel', finishDrawing, true);

  copyButton.addEventListener('click', () => {
    copyJson();
  });

  window.addEventListener('hashchange', () => {
    refreshScreenOptions();
  });

  document.addEventListener('keydown', (event) => {
    if (isFormTarget(event.target)) {
      return;
    }
    if (event.key.toLowerCase() === TOGGLE_KEY) {
      setVisibility(!isVisible);
      event.preventDefault();
    } else if (event.key === 'Escape' && isVisible) {
      isDrawing = false;
      startPoint = null;
      currentRect = null;
      drawZone(null);
      copyButton.disabled = true;
      rectOutput.textContent = 'No zone drawn';
      event.preventDefault();
    }
  });

  refreshScreenOptions();
  setVisibility(true);
})();
