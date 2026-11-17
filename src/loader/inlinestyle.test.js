// The page itself keeps no inline <style> or inline <script>. Tauri hashes each one into the CSP
// when it builds, and a hash in style-src makes the browser ignore 'unsafe-inline': the <style>
// that xterm adds at run time is then blocked and every Log (HOME and DEV) loses its font, its
// colours and its grid. 0.2.15's start-up screen did exactly that; its look now lives in
// public/loader/loader.css.
import { describe, expect, it } from 'vitest'
import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../..', import.meta.url))
const page = readFileSync(`${root}index.html`, 'utf8')

describe('index.html', () => {
  it('has no inline <style> block for Tauri to hash into the CSP', () => {
    expect(page).not.toMatch(/<style[\s>]/i)
  })

  it('has no inline <script> body either (only scripts with a src)', () => {
    const inline = [...page.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)].filter(([, attrs, body]) => !/\bsrc=/.test(attrs) || body.trim())
    expect(inline).toEqual([])
  })

  it('links the start-up screen look from a file that exists', () => {
    expect(page).toContain('href="/loader/loader.css"')
    expect(existsSync(`${root}public/loader/loader.css`)).toBe(true)
  })

  it("keeps 'unsafe-inline' in style-src, which xterm's run-time <style> needs", () => {
    const conf = JSON.parse(readFileSync(`${root}src-tauri/tauri.conf.json`, 'utf8'))
    expect(conf.app.security.csp).toMatch(/style-src[^;]*'unsafe-inline'/)
  })
})
