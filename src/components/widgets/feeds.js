// HOME's FEEDS card: status lists the companion keeps in `<memory folder>/feeds/*.json`.

// A feed not written for this long is shown as stale (its writer has probably stopped).
export const STALE_MS = 3 * 60 * 1000

// Is this feed stale at `now`? A feed with no time, or one Castle cannot read, is not judged.
export function isStale(feed, now = Date.now()) {
  if (!feed?.updated) return false
  const at = new Date(feed.updated).getTime()
  if (Number.isNaN(at)) return false
  return now - at > STALE_MS
}

// The short line under a stale feed's title.
export function staleNote(feed, now = Date.now()) {
  if (!isStale(feed, now)) return ''
  const minutes = Math.round((now - new Date(feed.updated).getTime()) / 60000)
  return minutes < 120 ? `not updated for ${minutes} min` : `not updated for ${Math.round(minutes / 60)} h`
}

// Is any switch in these feeds still moving (the writer says busy)?
export function anyBusy(feeds) {
  return (feeds || []).some((f) => (f.rows || []).some((r) => r.switch === 'busy'))
}
