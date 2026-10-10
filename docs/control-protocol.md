# Control protocol

Start the app with `--control <port>`. The server listens on `127.0.0.1` only and speaks JSON
lines: one request object per line, one reply per line.

```json
{"id": 1, "method": "engine.execute", "params": {"command": "shape.insert", "params": {"preset": "roundRect", "rect": [100, 100, 240, 120], "text": "Hello"}}}
{"id": 1, "ok": true, "result": {"id": 258}}
```

Coordinates are slide points (1/72 inch; a 16:9 slide is 960 × 540) unless noted.

| Method | Params | What it does |
|---|---|---|
| `engine.execute` / `ui.menu.invoke` / `command` | `{command, params}` | Run any command (see `engine.commands`; `deckcraft-cli describe ID` documents one) |
| `engine.commands` | — | Every command: id, label, menu path, shortcut, params doc, enabled / disabled reason |
| `document.inspect` | — | Slides, sections, layouts, shapes, selection |
| `ui.inspect` | — | UI state, active tool, window/canvas/slide rects, dialog, slide show state, documents, perf (fps, render ms) |
| `ui.tool.select` | `{tool, preset?}` | `select`, `text`, `shape` + preset, any preset name, `curve`, `freeform`, `scribble`, `pen`, `highlighter`, `eraser` |
| `ui.pointer` | `{events: [{kind: down\|drag\|up\|move\|doubleclick\|tripleclick, x, y, mods?}], mods?}` | Drive the active tool in slide points through the same code path as the mouse |
| `ui.key` / `ui.text` | `{key, shift?, alt?, cmd?}` / `{text}` | Keyboard input (also drives a running slide show) |
| `ui.move` / `ui.click` / `ui.drag` | screen points | Real egui pointer input — reaches every widget, menu and pane |
| `ui.set` | `{tab?, view?, pane?, notes?, zoom?, brightness?, ruler?, gridlines?, guides?, …}` | UI state (returns the full state) |
| `ui.dialog.open` / `ui.dialog.close` | `{id, fields?}` | Open or close a dialog by id; `fields` pre-fills its fields (About: `{"tab": "contributors"}` or `"models"`) |
| `ui.screenshot` | `{path?}` | PNG of the whole window (base64 when no path) |
| `ui.render` | `{path?, slide?, scale?}` | Render a slide headlessly |
| `ui.resize` / `ui.focus` | | Window control |
| `show.start` / `show.end` | `{from?: slide index, presenter?, reading?}` | Slide show |
| `app.open` / `app.save` / `app.export` / `app.quit` | `{path, …}` | Files (export takes the `file.export` params, e.g. `{"path": "deck.pdf", "layout": "handouts", "perPage": 3}`) |

The MCP server (`deckcraft-cli mcp`, see [mcp.md](mcp.md)) wraps the same methods for Claude
and other agents; `deckcraft-cli app METHOD [JSON]` sends one request from a shell.

### IME composition

Canvas input uses `engine.execute` with `text.preedit {text}`, `text.commit {text}` and
`text.cancelComposition {}`. Repeated preedit updates replace a disposable canvas preview;
the saved document, dirty flag, undo history and command journal remain unchanged. A commit
inserts the final string once and creates one undo step. Empty preedit hides the preview while
retaining its target for the following commit; empty commit cancels it without deleting the
original selection. Moving the selection or switching documents invalidates a pending composition
and interrupts the platform IME, so a late commit cannot type into the new target. Ignored commits
are excluded from the command journal. Native and web input
methods are enabled while a text-capable canvas selection owns keyboard focus; the rendered caret
geometry is reported to the backend. Focused UI text fields (notes, dialogs) own their own
IME events.

Application shortcuts and clipboard actions first finish a live composition. A backend Commit
already queued in the same input batch takes precedence over its preview (conversion candidates
can differ). Otherwise `text.commit {text, finish: true}` commits the preview locally and ignores
one matching backend echo; the committed string is journaled, so replay and undo stay consistent.
New nonempty preedit, direct typing or a real surrounding correction starts fresh input.

Empty preedit alone never swallows ordinary keys. When it clears a live composition, its
confirmation/deletion key is suppressed for that input batch, including an empty Commit on web
backends. A subsequent ordinary canvas key calls `text.dismissComposition {}` to release the
ended preedit before navigating. This explicit input boundary allows a new commit-only session,
even with the same text. It does not release a composition invalidated by a document/selection
change or explicit cancellation. IME events have no session identifiers, so this boundary is
inferred from ordinary input; integrations should not send an old Commit after that new input.
After a real Commit only its confirming Enter is suppressed, leaving subsequent navigation and
deletion available. IME line-break notifications are ignored; ordinary Enter handles line breaks.

`text.deleteSurrounding {beforeChars, afterChars}` supports mobile IME recomposition and corrections.
Counts are Unicode characters outside the current selection, including paragraph breaks; the
selected text itself is preserved. Deletion is a single undoable edit and updates any active
composition's target and preview.
