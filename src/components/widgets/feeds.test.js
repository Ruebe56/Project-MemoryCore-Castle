import { describe, expect, it } from 'vitest'
import { STALE_MS, anyBusy, isStale, staleNote } from './feeds.js'

const now = new Date('2026-11-16T10:30:00+08:00').getTime()

describe('feeds', () => {
  it('a fresh feed is not stale', () => {
    expect(isStale({ updated: '2026-11-16T10:29:30+08:00' }, now)).toBe(false)
    expect(staleNote({ updated: '2026-11-16T10:29:30+08:00' }, now)).toBe('')
  })
  it('an old feed is stale, with minutes then hours', () => {
    expect(isStale({ updated: new Date(now - STALE_MS - 1000).toISOString() }, now)).toBe(true)
    expect(staleNote({ updated: '2026-11-16T10:20:00+08:00' }, now)).toBe('not updated for 10 min')
    expect(staleNote({ updated: '2026-11-16T07:30:00+08:00' }, now)).toBe('not updated for 3 h')
  })
  it('no time or a bad time is never judged', () => {
    expect(isStale({ updated: '' }, now)).toBe(false)
    expect(isStale({ updated: 'soon' }, now)).toBe(false)
    expect(isStale(null, now)).toBe(false)
  })
  it('anyBusy sees a moving switch and nothing else', () => {
    expect(anyBusy([{ rows: [{ switch: 'on' }, { switch: 'busy' }] }])).toBe(true)
    expect(anyBusy([{ rows: [{ switch: 'on' }, { switch: 'off' }, {}] }])).toBe(false)
    expect(anyBusy(null)).toBe(false)
  })
})
