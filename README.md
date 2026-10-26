<div align="center">
  <img src="src-tauri/icons/128x128@2x.png" alt="deep-code-pet icon" width="96" height="96">
  <h1>deep-code-pet</h1>
  <p>
    <strong>A desktop pet that lives on your Mac</strong>
    <br>
    Animated from local sprite assets, powered by the agent you already run in the terminal.
  </p>
  <p>
    <a href="https://github.com/liwenka1/deep-code-pet/releases">
      <img src="https://img.shields.io/github/v/release/liwenka1/deep-code-pet?style=flat-square" alt="Release">
    </a>
    <a href="LICENSE">
      <img src="https://img.shields.io/github/license/liwenka1/deep-code-pet?style=flat-square" alt="License">
    </a>
    <img src="https://img.shields.io/badge/-macOS-black?style=flat-square&logo=apple&logoColor=white" alt="macOS">
  </p>
  <p>
    <a href="https://github.com/liwenka1/deep-code-pet/releases">Releases</a> •
    <a href="README.zh-CN.md">中文说明</a>
  </p>
</div>

---

## Overview

**deep-code-pet** is a small always-on-top desktop pet. It reads animal sprite sheets from a local
folder, animates them with a nine-state machine, and talks through the `deepcode` CLI you already
have installed — same account, same configuration, no extra key.

- **Local-first**: sprites, persona and conversation history all live in your own folders
- **No bundled model**: the pet shells out to your local `deepcode`; the app itself stores no credentials
- **Separate windows**: the pet is a transparent overlay, chat and history are their own panels, so
  opening a panel never moves the pet

## Highlights

| Feature | Description |
| :-- | :-- |
| **Nine-state sprite animation** | Idle, running, waving, jumping, failed, waiting, review and two directions, with per-frame timing taken from the asset format. |
| **Drag to swap pets** | Drop any pet folder on the window; the choice is remembered across launches. |
| **Chat panel** | Full transcript, multi-line input, streaming replies, scrollback and text selection. |
| **Conversation history** | A list window to switch, start and inspect conversations; each one is a file on disk. |
| **Real agent, not a mock** | Tool calls, errors and turn ends all drive the animation; text goes to the chat panel. |
| **Multi-monitor dragging** | Window position comes from the OS cursor, so dragging works across displays with mixed scaling. |
| **Above the menu bar** | The pet window is raised to `NSStatusWindowLevel`, so it can sit at the very top of the screen. |
| **Tiny footprint** | Transparent frameless window that hugs the sprite; nothing but the pet is clickable. |

## How it works

The pet is a state machine driven by the agent's event stream:

| Event | Pet | Text |
| :-- | :-- | :-- |
| turn started / reasoning / assistant delta | `running` | chat panel (streaming) |
| tool started | `running` | one-line note in the chat panel |
| turn finished with tool calls | `review` → `idle` | reply text in the chat panel |
| turn finished, pure chat | `jumping` → `idle` | reply text in the chat panel |
| error / non-zero exit / timeout | `failed` → `idle` | reason shown once |
| turn cancelled | `idle` | one-line note |
| calls auto-denied (nobody to ask) | `waiting` → `idle` | one-line note |

`waving` is not tied to an event: it plays on launch, when you call the pet, when a new pet is
loaded and when a conversation changes.

### Interactions

| Action | Result |
| :-- | :-- |
| Click | Hop once (delayed 300 ms so a double click never hops first) |
| Double click / hover button | Open the chat panel |
| Right click | Native menu: history, new conversation, open chat, quit |
| Drag | Run in the drag direction; the window follows the cursor |
| `Cmd/Ctrl` + `-` / `=` | Resize the pet (0.6–1.6×), remembered |
| `Cmd/Ctrl` + `Shift` + `R` | Start a new conversation (history is kept) |
| `Esc` | Dismiss the notice card / input |

## Assets

A pet is one folder with two files:

```
<pet>/
├── pet.json            metadata (5 fields)
└── spritesheet.webp    atlas, or .png
```

| Item | Value |
| :-- | :-- |
| Frame size / columns | 192×208, **always 8 columns**, frames left-aligned |
| v1 atlas | 1536×1872 (9 rows) |
| v2 atlas | 1536×2288 (11 rows; the extra 2 rows are free for consumers) |
| Minimum | image ≥ 256×256 |

Nine fixed rows, in this order: `idle`, `running-right`, `running-left`, `waving`, `jumping`,
`failed`, `waiting`, `running`, `review`. Frame counts and per-frame durations are part of the
format; `idle` deliberately uses uneven timing for its blink.

```json
{
  "id": "demo",
  "displayName": "Demo",
  "description": "One line, optional.",
  "spriteVersionNumber": 2,
  "spritesheetPath": "spritesheet.webp"
}
```

`spriteVersionNumber` selects 9 or 11 rows (missing means v1); unknown fields are ignored.
Three ways to install a pet: drag the folder onto the window, drop it into
`~/.deep-code-pet/pets/<name>/`, or point `DEEP_CODE_PET_ASSET` at it.

## Requirements

- macOS (the only platform exercised so far)
- Node.js 20+ and pnpm
- [Rust toolchain](https://rustup.rs) for building the app
- `deepcode` installed and configured (`deepcode --version` works, `deepcode doctor` reports a key)

The app spawns `deepcode -p <prompt> --output-format stream-json` and inherits your environment, so
billing goes to the same account and no API key is ever stored by the pet.

## Download

Grab the latest `.dmg` from [Releases](https://github.com/liwenka1/deep-code-pet/releases).

Because the build is currently unsigned, macOS blocks it on first launch:

```bash
xattr -cr /Applications/deep-code-pet.app
```

Run that once, then open the app again.

## Local development

```bash
pnpm install      # dependencies
pnpm test         # offline self-tests (45 assertions, no network, no Tauri)
pnpm app          # tauri dev
pnpm build        # frontend bundle only
```

Rust checks mirror CI:

```bash
cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## CI and release

Every push to `main` and every pull request runs two jobs in
[`.github/workflows/ci.yml`](.github/workflows/ci.yml): `frontend (checks)` (syntax, self-tests,
Vite build) and `rust (checks)` (rustfmt, clippy with `-D warnings`, cargo test,
`tauri build --no-bundle`).

Releasing is a manual dispatch: **Actions → Release → Run workflow**, enter a version without the
`v` prefix. It validates semver, requires `main`, writes the version into four files, commits, tags,
builds both macOS architectures and uploads one dmg each to the same GitHub Release.

## Known limitations

1. **No click-through** — the window hugs the sprite plus an 8px margin, but that margin still
   swallows clicks.
2. **No speech bubble** — all text lives in the chat panel; short notices use a card that fades after
   four seconds.
3. **One process per turn** — each message spawns `deepcode -p` (roughly 0.1–0.3 s plus model
   latency). Switching to a resident `deepcode serve` + SSE would not touch the reducer.
4. **Approvals cannot be answered** — headless runs auto-deny, so `approval.required` never arrives;
   the pet falls back to `denied_approvals` for a hint.
5. **macOS only** — Windows and Linux are untested.
6. **No pet picker UI** — swap by dragging, or edit `~/.deep-code-pet/pets`.

## License

[MIT](LICENSE)
