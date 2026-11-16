<script setup>
// HOME's FEEDS card: one small status list per `<memory folder>/feeds/*.json`. The companion (or
// any script) writes those files; Castle only reads them, every 20 seconds while HOME is visible.
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { memoryFeeds, feedSwitch } from '../../api/castle.js'
import { staleNote, anyBusy } from './feeds.js'

const feeds = ref(null)
const now = ref(Date.now())
const pressed = ref({}) // "feed/id" -> true while a press waits for the feed to answer
const error = ref('')
let timer
let quick

async function read() {
  now.value = Date.now()
  if (document.visibilityState !== 'visible') return
  try {
    feeds.value = await memoryFeeds()
  } catch {
    // The last reading stays.
  }
  // While a switch is moving, look again every 3 seconds instead of every 20.
  const moving = anyBusy(feeds.value) || Object.keys(pressed.value).length > 0
  if (moving && !quick) quick = setInterval(read, 3000)
  if (!moving && quick) {
    clearInterval(quick)
    quick = null
  }
}

// The feed's own answer replaces the press once it says busy or has flipped.
function switchOf(f, r) {
  const key = `${f.name}/${r.id}`
  return pressed.value[key] ? 'busy' : r.switch
}

async function flip(f, r) {
  const key = `${f.name}/${r.id}`
  if (switchOf(f, r) === 'busy') return
  error.value = ''
  pressed.value = { ...pressed.value, [key]: true }
  try {
    await feedSwitch(f.name, r.id, r.switch !== 'on')
  } catch (e) {
    error.value = String(e)
  }
  // Give the feed's writer a moment to pick the request up, then trust only what it says.
  setTimeout(() => {
    const { [key]: _, ...rest } = pressed.value
    pressed.value = rest
    read()
  }, 2500)
  read()
}

onMounted(() => {
  read()
  timer = setInterval(read, 20000)
})
onBeforeUnmount(() => {
  clearInterval(timer)
  clearInterval(quick)
})
</script>

<template>
  <section class="widget card feeds">
    <p v-if="!feeds" class="empty">Checking…</p>
    <p v-else-if="!feeds.length" class="empty">No feeds yet. A companion can write them to feeds/*.json in the memory folder.</p>
    <div v-for="f in feeds" :key="f.name" class="feed" :class="{ stale: staleNote(f, now) }">
      <h3><span class="icon">◉</span>{{ f.title }}<span v-if="staleNote(f, now)" class="note">{{ staleNote(f, now) }}</span></h3>
      <ul>
        <li v-for="r in f.rows" :key="r.label" :class="r.state">
          <span class="lamp" :aria-label="r.state" />
          <span class="label">{{ r.label }}</span>
          <span class="detail">{{ r.detail }}</span>
          <button
            v-if="r.switch"
            type="button"
            class="switch"
            :class="switchOf(f, r)"
            role="switch"
            :aria-checked="r.switch === 'on'"
            :disabled="switchOf(f, r) === 'busy'"
            :title="switchOf(f, r) === 'busy' ? 'Switching…' : r.switch === 'on' ? `Turn ${r.label} off` : `Turn ${r.label} on`"
            @click="flip(f, r)"
          ><span class="knob" /></button>
          <span v-else />
        </li>
      </ul>
    </div>
    <p v-if="error" class="err">{{ error }}</p>
  </section>
</template>

<style scoped>
.card {
  padding: 14px 18px;
  box-sizing: border-box;
}
h3 {
  display: flex;
  align-items: baseline;
  gap: 10px;
  margin: 0 0 8px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--line);
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.14em;
}
.feed + .feed h3 {
  margin-top: 12px;
}
.icon {
  color: var(--accent);
}
.note {
  margin-left: auto;
  font-size: 11px;
  letter-spacing: 0;
  color: var(--text-dim);
}
ul {
  display: grid;
  gap: 5px;
  margin: 0;
  padding: 0;
  list-style: none;
}
li {
  display: grid;
  grid-template-columns: 10px 6.5em minmax(0, 1fr) 26px;
  gap: 10px;
  align-items: center;
  font-size: 13px;
}
.switch {
  position: relative;
  width: 26px;
  height: 14px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 7px;
  background: transparent;
  cursor: pointer;
  transition: background 0.15s;
}
.switch .knob {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-dim);
  transition: left 0.15s;
}
.switch.on {
  background: color-mix(in srgb, var(--accent) 35%, transparent);
  border-color: var(--accent);
}
.switch.on .knob {
  left: 14px;
  background: var(--accent);
}
.switch.busy {
  cursor: wait;
  opacity: 0.6;
}
.switch.busy .knob {
  left: 8px;
  animation: blink 0.9s ease-in-out infinite alternate;
}
.switch:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: 2px;
}
@keyframes blink {
  to {
    opacity: 0.3;
  }
}
.err {
  margin: 8px 0 0;
  font-size: 11px;
  color: #e06a6a;
}
.lamp {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-dim);
  opacity: 0.5;
}
.on .lamp {
  background: #5ee08f;
  opacity: 1;
  box-shadow: 0 0 6px #5ee08f;
}
.warn .lamp {
  background: #f2c14e;
  opacity: 1;
}
.off .lamp {
  background: #e06a6a;
  opacity: 0.9;
}
.detail {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  color: var(--text-dim);
  font-size: 12px;
}
.stale li {
  opacity: 0.55;
}
.empty {
  margin: 0;
  font-size: 13px;
  color: var(--text-dim);
}
</style>
