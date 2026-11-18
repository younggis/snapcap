/**
 * 真实 Tauri UI 端到端回归（WebView2 CDP）：
 * 主窗口点击“窗口” → 覆盖层 mode=window → 识别并点击真实 Win32 窗口 →
 * 真实键盘 Enter → 检查截图落盘 → 标注编辑器显示图像与保存路径。
 *
 * 用法: node cdp_window_flow.mjs <port> <target.json> [expected-out-dir] [artifacts-dir]
 * 退出码 0 表示全链路通过。
 */
import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'

const port = Number(process.argv[2] || 9225)
const targetInfo = JSON.parse(fs.readFileSync(process.argv[3], 'utf8').replace(/^\uFEFF/, ''))
const expectedOutDir = process.argv[4] ? path.resolve(process.argv[4]) : ''
const artifactsDir = process.argv[5] ? path.resolve(process.argv[5]) : ''
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function targets() {
  const res = await fetch(`http://127.0.0.1:${port}/json/list`)
  if (!res.ok) throw new Error(`CDP list HTTP ${res.status}`)
  return res.json()
}

async function waitTarget(match, timeout = 20000) {
  const deadline = Date.now() + timeout
  while (Date.now() < deadline) {
    const list = await targets()
    const hit = list.find(match)
    if (hit) return hit
    await sleep(120)
  }
  throw new Error('CDP target not found')
}

function connect(wsUrl) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(wsUrl)
    const pending = new Map()
    let id = 0
    ws.addEventListener('open', () => resolve({
      send(method, params = {}) {
        return new Promise((res, rej) => {
          const msgId = ++id
          pending.set(msgId, { res, rej })
          ws.send(JSON.stringify({ id: msgId, method, params }))
        })
      },
      close() { ws.close() },
      ws,
    }))
    ws.addEventListener('message', (event) => {
      const msg = JSON.parse(String(event.data))
      if (!msg.id) return
      const p = pending.get(msg.id)
      if (!p) return
      pending.delete(msg.id)
      if (msg.error) p.rej(new Error(`${msg.error.message} (${JSON.stringify(msg.error)})`))
      else p.res(msg.result)
    })
    ws.addEventListener('error', () => reject(new Error('CDP websocket error')))
  })
}

async function evaluate(cdp, expression) {
  const result = await cdp.send('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  })
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text)
  }
  return result.result.value
}

async function waitEval(cdp, expression, timeout = 20000) {
  const deadline = Date.now() + timeout
  let last
  while (Date.now() < deadline) {
    try {
      last = await evaluate(cdp, expression)
      if (last) return last
    } catch (e) {
      last = String(e)
    }
    await sleep(120)
  }
  throw new Error(`condition timed out: ${expression}; last=${JSON.stringify(last)}`)
}

function pngSize(path) {
  const b = fs.readFileSync(path)
  if (b.length < 24 || b.toString('ascii', 1, 4) !== 'PNG') throw new Error('not a PNG')
  return { width: b.readUInt32BE(16), height: b.readUInt32BE(20), bytes: b.length }
}

function realEnter() {
  execFileSync('powershell.exe', [
    '-NoProfile',
    '-Command',
    "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')",
  ], { timeout: 10000 })
}

try {
  console.log(`TARGET hwnd=${targetInfo.hwnd} rect=${targetInfo.x},${targetInfo.y} ${targetInfo.width}x${targetInfo.height}`)

  const mainTarget = await waitTarget((t) => t.type === 'page' && t.url.includes('view=settings'))
  const main = await connect(mainTarget.webSocketDebuggerUrl)
  await main.send('Runtime.enable')

  // The URL is available before Vue mounts and before the Tauri bridge is
  // ready; wait for real entry cards to avoid clicking an inert page.
  const readyState = await waitEval(main, `(() => {
    const cards = document.querySelectorAll('button.entry-card');
    const state = {
      cards: cards.length,
      tauri: typeof window.__TAURI_INTERNALS__ === 'object',
      ready: document.readyState,
    };
    return state.cards >= 4 && state.tauri &&
      (state.ready === 'interactive' || state.ready === 'complete') ? state : null;
  })()`, 15000)
  console.log(`MAIN ready=${readyState.ready} cards=${readyState.cards} tauri=${readyState.tauri}`)

  // The window URL/cards appear before Tauri managed `AppState` is available.
  // Invoke a harmless state-backed command until it succeeds; otherwise the
  // first capture click could fail with "state not managed".
  await waitEval(main, `(async () => {
    try {
      await window.__TAURI_INTERNALS__.invoke('get_settings');
      return true;
    } catch (_) {
      return null;
    }
  })()`, 20000)
  console.log('BACKEND ready')

  // The overlay webview is created at startup but may still be loading its
  // initial page. Wait for it to be ready, otherwise the first navigation can
  // take several seconds while WebView2 initializes.
  const overlayWarmTarget = await waitTarget((t) => t.url.includes('view=overlay'), 20000)
  const overlayWarm = await connect(overlayWarmTarget.webSocketDebuggerUrl)
  await overlayWarm.send('Runtime.enable')
  await waitEval(overlayWarm, `document.readyState === 'complete'`, 20000)
  overlayWarm.close()
  console.log('OVERLAY prewarmed')

  const clickWindowEntry = () => evaluate(main, `(() => {
    const b = [...document.querySelectorAll('button.entry-card')].find(
      (x) => x.textContent.includes('窗口') && x.textContent.includes('Alt')
    );
    if (!b) return false;
    b.click();
    return true;
  })()`)

  let overlaySeen = false
  for (let attempt = 0; attempt < 2 && !overlaySeen; attempt += 1) {
    if (attempt > 0) console.log('RETRY window entry click')
    const preToast = await evaluate(main, `document.querySelector('.ios-toast')?.textContent?.trim() || ''`)
    if (preToast) console.log(`CLICK pre-toast=${JSON.stringify(preToast)}`)
    if (!(await clickWindowEntry())) throw new Error('window capture entry button not found')
    await sleep(800)
    const earlyToast = await evaluate(main, `document.querySelector('.ios-toast')?.textContent?.trim() || ''`)
    console.log(`CLICK attempt ${attempt + 1} toast=${JSON.stringify(earlyToast)}`)
    const deadline = Date.now() + 15000
    while (Date.now() < deadline) {
      const list = await targets()
      if (list.some((t) => t.url.includes('view=overlay') && t.url.includes('mode=window'))) {
        overlaySeen = true
        break
      }
      await sleep(150)
    }
    if (!overlaySeen) {
      const toast = await evaluate(main, `document.querySelector('.ios-toast')?.textContent?.trim() || ''`)
      console.log(`CLICK attempt ${attempt + 1} did not open overlay; toast=${JSON.stringify(toast)}`)
    }
  }
  if (!overlaySeen) throw new Error('overlay never navigated to mode=window')
  console.log('CLICK entry = ok')

  await waitTarget((t) => t.url.includes('view=overlay') && t.url.includes('mode=window'), 10000)
  const overlayTarget = await waitTarget((t) => t.url.includes('view=overlay') && t.url.includes('mode=window'))
  const overlay = await connect(overlayTarget.webSocketDebuggerUrl)
  await overlay.send('Runtime.enable')
  const overlayState = await waitEval(overlay, `(() => {
    const root = document.querySelector('.overlay-root');
    if (!root || root.dataset.mode !== 'window') return null;
    const windows = Number(root.dataset.windowCount || 0);
    if (windows < 1) return null;
    return {
      mode: root.dataset.mode,
      windows,
      tag: document.querySelector('.mode-tag')?.textContent?.trim() || '',
      chip: document.querySelector('[data-testid="active-window"]')?.textContent?.trim() || '',
    };
  })()`)
  if (overlayState.mode !== 'window') throw new Error(`overlay mode=${overlayState.mode}`)
  if (overlayState.windows < 1) throw new Error('overlay recognized 0 windows')
  console.log(`OVERLAY mode=window detected=${overlayState.windows} tag=${JSON.stringify(overlayState.tag)}`)

  const dpr = await evaluate(overlay, 'window.devicePixelRatio || 1')
  const x = (targetInfo.x + targetInfo.width / 2) / dpr
  const y = (targetInfo.y + targetInfo.height / 2) / dpr
  await overlay.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y })
  await sleep(160)
  const hoverText = await evaluate(overlay, `document.querySelector('[data-testid="active-window"]')?.textContent?.trim() || ''`)
  console.log(`HOVER ${JSON.stringify(hoverText)} at ${x.toFixed(1)},${y.toFixed(1)} dpr=${dpr}`)
  if (!hoverText.includes(targetInfo.title)) {
    throw new Error(`hover chip did not show window title: ${JSON.stringify(hoverText)}`)
  }
  if (artifactsDir) {
    try {
      const viewport = await evaluate(overlay, '({ w: window.innerWidth, h: window.innerHeight })')
      const shot = await overlay.send('Page.captureScreenshot', {
        format: 'png',
        clip: {
          x: 0,
          y: 0,
          width: Math.min(1600, viewport.w),
          height: Math.min(900, viewport.h),
          scale: 1,
        },
      })
      const file = path.join(artifactsDir, 'window-overlay-proof.png')
      fs.writeFileSync(file, Buffer.from(shot.data, 'base64'))
      console.log(`OVERLAY PROOF ${file}`)
    } catch (e) {
      console.log(`OVERLAY PROOF skipped: ${e.message}`)
    }
  }
  await overlay.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', buttons: 1, clickCount: 1 })
  await overlay.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', buttons: 0, clickCount: 1 })

  const selected = await waitEval(overlay, `document.querySelector('.overlay-root')?.dataset.selectedHwnd || ''`)
  await sleep(150)
  const selectedTitle = await evaluate(overlay, `document.querySelector('[data-testid="active-window"]')?.textContent?.trim() || ''`)
  console.log(`SELECTED hwnd=${selected} title=${JSON.stringify(selectedTitle)}`)
  if (String(selected) !== String(targetInfo.hwnd)) {
    throw new Error(`selected hwnd=${selected}, expected=${targetInfo.hwnd}; hover=${JSON.stringify(hoverText)}`)
  }
  if (!selectedTitle.includes(targetInfo.title)) {
    throw new Error(`selected chip did not show target title: ${JSON.stringify(selectedTitle)}`)
  }

  realEnter()
  console.log('ENTER sent through real keyboard')

  const editorTarget = await waitTarget((t) => t.url.includes('view=editor'), 10000)
  const editor = await connect(editorTarget.webSocketDebuggerUrl)
  await editor.send('Runtime.enable')
  const editorState = await waitEval(editor, `(() => {
    const canvas = document.querySelector('canvas');
    const saved = document.querySelector('.saved-path')?.textContent?.trim() || '';
    const hiddenEmpty = !!document.querySelector('.empty');
    const visible = !!canvas && canvas.width > 1;
    if (hiddenEmpty || !visible || !saved) return null;
    return {
      hiddenEmpty,
      canvasWidth: canvas.width,
      canvasHeight: canvas.height,
      saved,
      visible,
    };
  })()`, 30000)
  console.log(`EDITOR empty=${editorState.hiddenEmpty} canvas=${editorState.canvasWidth}x${editorState.canvasHeight}`)
  console.log(`SAVED ${JSON.stringify(editorState.saved)}`)
  if (editorState.hiddenEmpty || !editorState.visible) throw new Error('editor has no visible screenshot')
  if (!editorState.saved) throw new Error('editor did not receive auto-saved path')

  // The editor can open with a canvas while the bitmap itself is fully
  // transparent (GDI alpha bug). Read back actual canvas pixels so that
  // "dimensions are right but the image is blank" fails the acceptance test.
  const painted = await waitEval(editor, `(() => {
    const cv = document.querySelector('canvas');
    if (!cv || cv.width < 2) return null;
    const data = cv.getContext('2d').getImageData(0, 0, cv.width, cv.height).data;
    let visible = 0, samples = 0;
    for (let i = 3; i < data.length; i += 4 * 32) { samples++; if (data[i] > 0) visible++; }
    const ratio = samples ? visible / samples : 0;
    return ratio > 0.10 ? { ratio } : null;
  })()`, 15000)
  console.log(`EDITOR pixels=${JSON.stringify(painted)}`)

  const savedPath = editorState.saved.replace(/^✓\s*/, '').trim()
  if (expectedOutDir) {
    const rel = path.relative(expectedOutDir, path.resolve(savedPath))
    if (!rel || rel.startsWith('..') || path.isAbsolute(rel)) {
      throw new Error(`saved outside isolated output dir: ${savedPath} (expected under ${expectedOutDir})`)
    }
  }
  const size = pngSize(savedPath)
  console.log(`PNG ${size.width}x${size.height} ${size.bytes}B`)
  if (size.width !== targetInfo.width || size.height !== targetInfo.height) {
    throw new Error(`PNG ${size.width}x${size.height} != window ${targetInfo.width}x${targetInfo.height}`)
  }
  if (artifactsDir) {
    const proof = path.join(artifactsDir, 'window-capture-proof.png')
    fs.copyFileSync(savedPath, proof)
    console.log(`CAPTURE PROOF ${proof}`)
  }

  console.log('E2E WINDOW FLOW PASS')
  overlay.close()
  editor.close()
  main.close()
  process.exit(0)
} catch (e) {
  console.error(`E2E WINDOW FLOW FAIL: ${e.stack || e}`)
  process.exit(1)
}
