# Edu-Harness File Formats

This document describes the two on-disk file formats used by edu-harness.

---

## Manifest file (`.edubook`)

A single JSON file that lives at the root of a book directory. It records metadata about the book and an ordered list of chapters.

**Source of truth:** `src-tauri/src/manifest.rs`, mirrored in `src/types/manifest.ts`

### Schema

```json
{
  "version": 1,
  "metadata": {
    "title": "Black Holes",
    "subtitle": "A brief introduction",
    "topic": "How black holes form and evolve",
    "prompt": "Teach me about black holes",
    "created": "2026-05-13T00:00:00Z",
    "modified": "2026-05-13T12:34:56Z",
    "description": "A comprehensive look at stellar-mass black holes.",
    "readingLevel": "adult",
    "priorKnowledge": "Basic high-school physics"
  },
  "lessonPlan": {
    "summary": "We begin with stellar evolution and work toward Hawking radiation.",
    "chapters": [
      {
        "id": "ch-01",
        "title": "Stellar Evolution",
        "description": "How stars live and die.",
        "file": "chapters/01-stellar-evolution.edupage",
        "status": "generated"
      }
    ]
  }
}
```

### Field reference

#### `metadata`

| Field | Type | Required | Notes |
|---|---|---|---|
| `title` | string | ✓ | Display title of the book |
| `subtitle` | string | — | Optional subtitle |
| `topic` | string | ✓ | Short topic description |
| `prompt` | string | ✓ | The original user prompt that generated the book |
| `created` | string | ✓ | ISO-8601 creation timestamp |
| `modified` | string | ✓ | ISO-8601 last-modified timestamp |
| `description` | string | — | Longer description of the book |
| `readingLevel` | string | — | e.g. `"child"`, `"teen"`, `"adult"`, `"academic"` |
| `priorKnowledge` | string | — | What the reader is expected to already know |

#### `lessonPlan`

| Field | Type | Required | Notes |
|---|---|---|---|
| `summary` | string | ✓ | Overall summary of the lesson plan |
| `chapters` | array | ✓ | Ordered list of chapters |

#### `chapters[*]`

| Field | Type | Required | Notes |
|---|---|---|---|
| `id` | string | ✓ | Stable identifier, e.g. `"ch-01"`. Appendix chapters use `"ap-N"`. |
| `title` | string | ✓ | Chapter display title |
| `description` | string | — | One-sentence summary of the chapter |
| `file` | string | ✓ | Path to the `.edupage` file, relative to the book directory |
| `status` | string | ✓ | `"planned"` \| `"generating"` \| `"generated"` |

---

## Chapter file (`.edupage`)

Each chapter is stored as a single plain-text file with a custom format that supports append-only revision history, inline notes, and embedded SVG artifacts — all in one file with no external assets.

**Source of truth:** `src-tauri/src/edupage.rs`

### Overall structure

```
<JSON header>
======! <file-id>|<sha1> !======
<markdown content block>
======! <file-id>|<sha1> !======
<edited line(s)>
======! <file-id>|NOTE:1 !======
<note body text>
======! <file-id>|ARTIFACT:1 !======
<SVG data>
```

The file is divided into **blocks** separated by delimiter lines. The first block is a JSON header; subsequent blocks are identified by the key that appears in their delimiter.

### Delimiter format

```
======! <file-id>|<key> !======
```

Where `<key>` is one of:

| Key pattern | Block contents |
|---|---|
| `<sha1-hex>` | A markdown content revision |
| `NOTE:<id>` | Body text for note with the given id |
| `ARTIFACT:<id>` | Raw SVG for artifact with the given id |

### Header (JSON)

```json
{
  "version": 1,
  "id": "ch-01",
  "title": "Stellar Evolution",
  "description": "How stars live and die.",
  "revisions": [
    {
      "id": "a3f2c1...",
      "ctime": "2026-05-13T00:00:00Z",
      "actionId": "550e8400-...",
      "type": "ADD",
      "lineStart": 1,
      "lineEnd": 42
    }
  ],
  "assets": [],
  "notes": [
    {
      "id": 1,
      "type": "definition",
      "word": "neutron star",
      "ctime": "2026-05-13T01:00:00Z"
    }
  ],
  "nextNoteId": 2,
  "nextRewriteId": 1,
  "artifacts": [
    {
      "id": 1,
      "mimeType": "image/svg+xml",
      "semanticType": "image",
      "ctime": "2026-05-13T02:00:00Z",
      "caption": "Cross-section of a neutron star",
      "aspectRatio": 1.78,
      "source": "neutron star"
    }
  ],
  "nextArtifactId": 2
}
```

#### Header field reference

| Field | Type | Notes |
|---|---|---|
| `version` | u32 | Format version; currently `1` |
| `id` | string | Matches the chapter `id` in the manifest |
| `title` | string | Chapter title |
| `description` | string? | Optional short description |
| `revisions` | array | Ordered list of content revisions (see below) |
| `assets` | array | Reserved for future binary assets |
| `notes` | array | Metadata for all notes in this chapter |
| `nextNoteId` | u32 | Monotonically increasing allocator; ids are never reused |
| `nextRewriteId` | u32 | Monotonically increasing allocator for rewrite spans |
| `artifacts` | array | Metadata for all SVG artifacts in this chapter |
| `nextArtifactId` | u32 | Monotonically increasing allocator; ids are never reused |

#### `revisions[*]`

| Field | Type | Notes |
|---|---|---|
| `id` | string | SHA-1 hex of the content block; used as the delimiter key |
| `ctime` | string | ISO-8601 timestamp |
| `actionId` | string | UUID identifying the action that created this revision |
| `type` | string | `"ADD"` or `"EDIT"` |
| `lineStart` | usize | 1-indexed first line this revision targets |
| `lineEnd` | usize? | 1-indexed last line (inclusive). `null` for open-ended |

#### `notes[*]`

| Field | Type | Notes |
|---|---|---|
| `id` | u32 | Unique within this file; never reused |
| `type` | string | `"definition"` \| `"footnote"` \| `"endnote"` |
| `word` | string | The word or phrase the note is attached to |
| `ctime` | string | ISO-8601 timestamp |

The body text of each note is stored in its own `NOTE:<id>` block, not in the header.

#### `artifacts[*]`

| Field | Type | Notes |
|---|---|---|
| `id` | u32 | Unique within this file; never reused |
| `mimeType` | string | Always `"image/svg+xml"` currently |
| `semanticType` | string | `"image"`, `"diagram"`, etc. |
| `ctime` | string | ISO-8601 timestamp |
| `caption` | string? | Optional display caption |
| `aspectRatio` | f32 | `width / height` from the SVG `viewBox`. Drives layout (see below). |
| `source` | string | The selected text that triggered artifact generation |

The SVG data for each artifact is stored in its own `ARTIFACT:<id>` block, not in the header.

### Reconstructing the markdown

Revisions are applied **in order** to build the final markdown document:

- **ADD** — inserts the block's lines starting at `lineStart` (1-indexed, clamped to document length).
- **EDIT** — replaces lines `lineStart..lineEnd` (inclusive, 1-indexed) with the block's lines.

The result is the rendered markdown that the reader sees.

### Inline anchors in the markdown

After reconstruction, certain inline markers appear in the text:

| Pattern | Meaning |
|---|---|
| `[^*N]` | Definition note with id N (asterisk marker) |
| `[^†N]` | Footnote with id N (dagger marker) |
| `[^‡N]` | Endnote with id N (double-dagger marker) |
| `[^AN]` | Cross-reference to appendix chapter `ap-N` |
| `![alt](epar://N)` | Artifact with id N |
| `<span data-rewrite-id="N">…</span>` | Rewritten passage with rewrite id N |

### Artifact placement

Artifacts are inserted into the markdown body relative to their `source` selection:

- **Wide** (`aspectRatio >= 1.0`) — placed as a block image on its own paragraph after the source line.
- **Tall** (`aspectRatio < 1.0`) — inserted inline immediately after the source text, so the renderer can float it beside the paragraph.

If an artifact is regenerated and its aspect ratio crosses the wide/tall boundary, a new EDIT revision repositions the body marker accordingly.

---

## Source-position annotation system

When the reader selects text and triggers an action (Define, Footnote, Diagram, Rewrite, …), the frontend must map that selection back to exact byte positions in the reconstructed markdown. This is done entirely through attributes embedded in the HTML — no text searching is performed.

**Source of truth:** `src-tauri/src/render.rs` (renderer), `src/lib/selection.ts` (frontend)

### How rendering works

`render_chapter_html(markdown, notes, artifacts)` in `render.rs` runs pulldown-cmark in offset mode (`into_offset_iter()`), which pairs every event with its byte range in the source string. The renderer then emits HTML with the following attributes on every element whose visible text maps to source bytes:

| HTML attribute | Meaning |
|---|---|
| `data-src-start="N"` | UTF-8 byte offset in the reconstructed markdown where this element's text begins |
| `data-src-end="N"` | UTF-8 byte offset where this element's text ends (exclusive) |
| `data-src-skip` | This element's visible content does **not** correspond to source bytes; the frontend skips its entire subtree |

`data-src-start`/`data-src-end` are placed on:
- An inline `<span>` wrapping each contiguous run of body text (after splitting on custom anchor markers — see below).
- Block-level elements (`<p>`, `<h1>`–`<h6>`, `<li>`, `<blockquote>`) receive the span of the whole block as a coarse anchor.

`data-src-skip` is placed on:
- Note-anchor superscripts (`<sup class="note-anchor" …>`) — the marker bytes (`[^*N]` etc.) are consumed and produce no selectable text.
- Appendix cross-reference links (`<a class="appendix-ref" …>`).
- `<figure>` elements wrapping inlined SVG artifacts.
- The endnote section (`<section class="endnotes" …>`) appended at the bottom of the chapter.

### Handling pulldown-cmark character splitting

pulldown-cmark splits text on `[` and similar characters that could begin inline markup. The input `"word[^*1] rest"` produces individual `Text` events for `"["`, `"^"`, `"*"`, `"1"`, `"]"` rather than a single `"[^*1]"` event.

`merge_adjacent_texts()` merges all consecutive `Text` events at contiguous byte positions before the rendering loop runs. After merging, `emit_text()` calls `split_anchors()` on the combined string to split out anchor markers from selectable text.

### Anchor processing in `emit_text`

`split_anchors(s)` walks the merged text string and splits out each `[^markerN]` pattern:

- Plain text segments → `<span data-src-start="A" data-src-end="B">…</span>`
- Anchor segments → `<sup data-src-skip …>` or `<a data-src-skip …>` with no source byte contribution to the offset map

The byte cursor advances through both plain and anchor segments so that `data-src-start` values for text after an anchor are always correct.

### Frontend selection flow

When the reader makes a text selection:

1. **`resolveSourceRange(range, articleEl)`** in `src/lib/selection.ts`:
   - Walks up from `range.startContainer` to find the nearest ancestor with `data-src-start`, returning `null` if a `data-src-skip` boundary is crossed first.
   - Computes `srcStart = spanSrcStart + TextEncoder.encode(spanText.slice(0, startCharOffset)).length` — `TextEncoder` produces UTF-8 bytes, matching Rust's native string indexing.
   - Does the same from `range.endContainer`, computing `srcEnd = spanSrcEnd − utf8ByteLen(trailingText)`.
   - Returns `{ srcStart, srcEnd }` or `null` if either endpoint is inside a skip zone or unannotated content.

2. The resolved byte range is sent directly to the Tauri backend command (e.g. `define_word`, `add_image`, `rewrite_passage`) as `src_start: usize, src_end: usize`.

3. Rust backend operations work at exact byte positions via `locate_byte(content, pos)` — no text searching.

### Why `null` is returned

`resolveSourceRange` returns `null` when:
- Either endpoint of the selection is inside a `data-src-skip` subtree (e.g. the user dragged from a note-anchor number into actual text).
- No `data-src-start` ancestor is found (the selection is outside the rendered chapter, e.g. inside the endnotes section).

The frontend shows an error message and does not invoke the backend command in these cases.
