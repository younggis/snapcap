/**
 * 标注编辑器文字标注端到端回归（WebView2 CDP）：
 * 主窗口点击“全屏” → 截图 → 编辑器打开 → 选文字工具 + 蓝色 → 输入文字 →
 * 真实键盘 Enter 提交 → 断言输入框关闭且画布出现蓝色像素（颜色生效）。
 *
 * 重点回归：Enter/Esc 全局快捷键曾常驻注册，会在系统层吞掉按键，导致文字
 * 输入框的 Enter 无法提交（表现为“添加文字标注不成功 / 颜色没生效”）。
 * 该脚本同时用 CDP 键盘（直达 DOM）和真实 SendKeys（走全局快捷键路径）验证。
 *
 * 用法: node cdp_text_flow.mjs <port>
 * 退出码 0 表示通过。
 */
import { execFileSync } from 'node:child_process'

const port = Number(process.argv[2] || 9225)
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function targets() {
  const res = await fetch(`http://127.0.0.1:${port}/json/list`)
  if (!res.ok) throw new Error(`CDP list HTTP ${res.status}`)
  return res.json()
}
async function waitTarget(match, timeout = 25000) {
  const deadline = Date.now() + timeout
  while (Date.now() < deadline) {
    const hit = (await targets()).find(match)
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
    }))
    ws.addEventListener('message', (event) => {
      const msg = JSON.parse(String(event.data))
      if (!msg.id) return
      const p = pending.get(msg.id)
      if (!p) return
      pending.delete(msg.id)
      msg.error ? p.rej(new Error(msg.error.message)) : p.res(msg.result)
    })
    ws.addEventListener('error', () => reject(new Error('CDP websocket error')))
  })
}
async function evaluate(cdp, expression) {
  const result = await cdp.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
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
    } catch (e) { last = String(e) }
    await sleep(120)
  }
  throw new Error(`condition timed out: ${expression}; last=${JSON.stringify(last)}`)
}
function realEnter() {
  execFileSync('powershell.exe', [
    '-NoProfile', '-Command',
    "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')",
  ], { timeout: 10000 })
}

const canvasPixels = `(() => {
  const c = document.querySelector('canvas')
  if (!c) return { blue: 0, red: 0 }
  const ctx = c.getContext('2d')
  const d = ctx.getImageData(0, 0, c.width, c.height).data
  let blue = 0, red = 0
  for (let i = 0; i < d.length; i += 4) {
    const r = d[i], g = d[i+1], b = d[i+2]
    if (b > 180 && r < 100 && g < 150) blue++
    if (r > 180 && g < 100 && b < 100) red++
  }
  return { blue, red }
})()`

try {
  const mainTarget = await waitTarget((t) => t.type === 'page' && t.url.includes('view=settings'))
  const main = await connect(mainTarget.webSocketDebuggerUrl)
  await main.send('Runtime.enable')
  await waitEval(main, `(async () => {
    try { await window.__TAURI_INTERNALS__.invoke('get_settings'); return true } catch (_) { return null }
  })()`, 20000)

  // 全屏截图 -> 编辑器
  if (!(await evaluate(main, `(() => {
    const b = [...document.querySelectorAll('button.entry-card')].find(x => x.textContent.includes('全屏'))
    if (!b) return false
    b.click(); return true
  })()`))) throw new Error('full-screen entry not found')

  const editorTarget = await waitTarget((t) => t.url.includes('view=editor'), 20000)
  const editor = await connect(editorTarget.webSocketDebuggerUrl)
  await editor.send('Runtime.enable')
  await waitEval(editor, `(() => {
    const c = document.querySelector('canvas')
    return c && c.width > 1 && !!document.querySelector('.saved-path')
  })()`, 30000)
  console.log('EDITOR ready')

  // 文字工具 + 蓝色
  if (!(await evaluate(editor, `(() => {
    const b = [...document.querySelectorAll('.tool')].find(x => x.textContent.trim() === '文字')
    if (!b) return false
    b.click(); return true
  })()`))) throw new Error('text tool not found')
  const tool = await evaluate(editor, `document.querySelector('.tool.active')?.textContent?.trim()`)
  if (tool !== '文字') throw new Error(`text tool not active: ${tool}`)
  if (!(await evaluate(editor, `(() => {
    const b = [...document.querySelectorAll('.swatch')].find(x => x.title === '蓝')
    if (!b) return false
    b.click(); return true
  })()`))) throw new Error('blue swatch not found')
  console.log('TOOL text, COLOR blue')

  async function addTextAt(x, y, text, useRealEnter) {
    await editor.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', buttons: 1, clickCount: 1 })
    await editor.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', buttons: 0, clickCount: 1 })
    await sleep(250)
    if (!(await evaluate(editor, `!!document.querySelector('.text-editor input')`))) {
      throw new Error('text input did not appear after canvas click')
    }
    await editor.send('Input.insertText', { text })
    await sleep(150)
    if (useRealEnter) realEnter()
    else {
      await editor.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13 })
      await editor.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13 })
    }
    await sleep(400)
    if (await evaluate(editor, `!!document.querySelector('.text-editor input')`)) {
      throw new Error(`text input still open after ${useRealEnter ? 'real' : 'CDP'} Enter`)
    }
  }

  const box = await evaluate(editor, `(() => {
    const c = document.querySelector('canvas')
    const r = c.getBoundingClientRect()
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
  })()`)

  // CDP 键盘直达 DOM：验证提交逻辑本身
  await addTextAt(box.x, box.y, 'CDP', false)
  const afterCdp = await evaluate(editor, canvasPixels)
  console.log(`CDP Enter -> blue=${afterCdp.blue}`)
  if (afterCdp.blue < 20) throw new Error('CDP Enter did not paint blue text')

  // 真实键盘：回归全局快捷键吞键问题（核心）
  await addTextAt(box.x + 80, box.y + 40, '真实键盘', true)
  const afterReal = await evaluate(editor, canvasPixels)
  console.log(`REAL Enter -> blue=${afterReal.blue}`)
  if (afterReal.blue <= afterCdp.blue) throw new Error('real Enter did not commit text (global hotkey swallowed it)')

  console.log('E2E TEXT FLOW PASS')
  editor.close(); main.close(); process.exit(0)
} catch (e) {
  console.error(`E2E TEXT FLOW FAIL: ${e.stack || e}`)
  process.exit(1)
}
