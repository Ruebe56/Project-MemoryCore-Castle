import { describe, expect, it } from 'vitest'
import { hideComments, parseBlocks, parseSpans } from './markdown.js'

describe('parseBlocks', () => {
  it('draws a table with its header and rows', () => {
    const blocks = parseBlocks('Sealed, three times.\n\n| Commit | What |\n|---|---|\n| 4430f2ec | Link Lv.21 |\n| 182deb1f | Its art |\n\nDone')
    expect(blocks.map((b) => b.kind)).toEqual(['p', 'table', 'p'])
    expect(blocks[1].head).toEqual(['Commit', 'What'])
    expect(blocks[1].rows).toEqual([
      ['4430f2ec', 'Link Lv.21'],
      ['182deb1f', 'Its art'],
    ])
  })

  it('leaves a lone pipe line as text when no rule follows', () => {
    expect(parseBlocks('| not a table |').map((b) => b.kind)).toEqual(['p'])
  })

  it('reads headings, lists with depth and boxes, quotes and rules', () => {
    const blocks = parseBlocks('# Plan\n## Phase 1\n- one\n  - two\n- [x] done\n- [ ] open\n1. first\n> said\n---')
    expect(blocks.map((b) => b.kind)).toEqual(['h1', 'h2', 'item', 'item', 'item', 'item', 'item', 'quote', 'rule'])
    expect(blocks[3].depth).toBe(1)
    expect(blocks[4]).toMatchObject({ box: true, checked: true, text: 'done' })
    expect(blocks[5]).toMatchObject({ box: true, checked: false, text: 'open' })
    expect(blocks[6].number).toBe('1.')
  })

  it('keeps code fences whole, diagrams and all', () => {
    const blocks = parseBlocks('look:\n```\n +---+\n | # |\n +---+\n```\nafter')
    expect(blocks[1]).toEqual({ kind: 'code', text: ' +---+\n | # |\n +---+' })
    expect(blocks[2].text).toBe('after')
  })

  it('joins plain lines into one paragraph and splits on blank lines', () => {
    expect(parseBlocks('a\nb\n\nc')).toEqual([
      { kind: 'p', text: 'a\nb' },
      { kind: 'p', text: 'c' },
    ])
  })
})

describe('parseSpans', () => {
  it('finds bold, italic, code and links as plain text', () => {
    expect(parseSpans('**Diary Saved** - `save diary` is *quiet*, see [the page](https://x.y)')).toEqual([
      { bold: true, text: 'Diary Saved' },
      { text: ' - ' },
      { code: true, text: 'save diary' },
      { text: ' is ' },
      { em: true, text: 'quiet' },
      { text: ', see ' },
      { text: 'the page', link: 'https://x.y' },
    ])
  })

  it('never turns markup into HTML', () => {
    expect(parseSpans('<img src=x onerror=alert(1)>')).toEqual([{ text: '<img src=x onerror=alert(1)>' }])
  })
})

describe('hideComments', () => {
  it('hides a spoken-line marker at the end of a reply', () => {
    const blocks = parseBlocks('It works.\n\n<!--speak: [calm]It works now.-->')
    expect(blocks).toEqual([{ kind: 'p', text: 'It works.' }])
  })

  it('hides a comment in the middle of a line and one across lines', () => {
    expect(hideComments('Hi there <!--speak: hello --> end')).toBe('Hi there  end')
    expect(hideComments('a\n<!-- one\ntwo -->\nb')).toBe('a\n\nb')
  })

  it('hides a comment still open while the reply streams in', () => {
    expect(hideComments('Done. <!--speak: [calm]Do')).toBe('Done. ')
  })

  it('keeps comments inside code blocks and inline code', () => {
    const text = 'Use `<!-- -->` to mark it.\n```html\n<!-- kept -->\n```\nafter <!--x--> end'
    expect(hideComments(text)).toBe('Use `<!-- -->` to mark it.\n```html\n<!-- kept -->\n```\nafter  end')
  })

  it('leaves text without comments unchanged', () => {
    const text = '# Title\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |'
    expect(hideComments(text)).toBe(text)
  })
})