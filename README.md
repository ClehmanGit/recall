# Recall v2

Local semantic search across AI chats and your files. One search box, one ranked list, one card per thing. Nothing leaves the machine.

```
recall/
├── core/          recall-core: ingest, rules, embed, index, search. No UI dependency.
│   └── src/bin/   probe: headless pipeline check
├── src-tauri/     Tauri 2 shell (window, commands, bundling, models/)
├── src/           frontend: vanilla TypeScript + Vite
│   └── views/     search, home, getting started, gallery and timeline, quick view, settings
├── site/          the one-page download website
├── scripts/       fetch-model.mjs, update-installed.ps1
└── assets/        mascot
```

## Shipping it

Nobody who installs Recall builds it. `.github/workflows/release.yml` builds the installer on GitHub's Windows runner and attaches it to a Release whenever you push a version tag:

```
git tag v2.1.1
git push origin v2.1.1
```

About fifteen minutes later the Release page has the installer twice, `Recall_<version>_x64-setup.exe` and the same file as `Recall-Setup.exe` (the fixed name the website links to), plus `SHA256SUMS.txt` with its fingerprint. Bump the version in `package.json`, `core/Cargo.toml`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` before tagging. The installer runs per-user, no admin prompt.

Every action in the workflows is pinned to a full commit and the model to a revision and SHA-256, so nothing upstream can quietly change what goes into the installer; Dependabot (`.github/dependabot.yml`) proposes updates as pull requests.

Code signing is not set up. To remove the SmartScreen warning, get a signing identity (Microsoft's Trusted Signing is the inexpensive route) and set `bundle.windows.signCommand` in `tauri.conf.json` to its signing command, with the credentials as repository secrets.

## The download page

`site/` is one page that fits on one screen: what Recall does, the download button and four privacy points, over a slow drift of files and chats (`site/drift.js`). The demo search types itself and the page scales to fit the window (`site/site.js`).

It uses nothing from anywhere else: fonts, images and scripts are in `site/`, and a Content-Security-Policy in the page blocks everything else. No cookies, no analytics. The download button goes to `releases/latest/download/Recall-Setup.exe`, so a new release needs no change to the page. The version and installer size in its text are typed in; update them with a release.

`.github/workflows/site.yml` swaps `REPLACE_ME/recall` for the repository's name and publishes `site/` to GitHub Pages, which serves it over HTTPS (enable Pages → Source: GitHub Actions in the repo settings once). To look at it locally: `python -m http.server 4321 --directory site`.

## Windows setup (for building it yourself)

Prerequisites, once:

1. Rust: https://rustup.rs (MSVC toolchain; say yes to Visual Studio Build Tools, "Desktop development with C++").
2. Node 20+: https://nodejs.org
3. WebView2 is already on Windows 10/11.

Then:

```powershell
npm install
npm run model      # bge-small-en-v1.5 int8, ~33 MB, into src-tauri/models/ — ships inside the installer
npm run dev        # builds the Rust side on first run (several minutes), then opens the window
```

`npm run build` produces the installer under `target/release/bundle/nsis/`. The model folder is bundled as a resource, so the installer is self-contained; no download on first launch.

To ship a feedback endpoint: `$env:RECALL_FEEDBACK_URL="https://your.endpoint/feedback"; npm run build`. Without it the feedback button is hidden.

### Updating your own installed copy

`scripts/update-installed.ps1` builds the app, closes the running Recall, swaps in the new `recall.exe` and starts it again. Your library and settings are untouched; no reinstall.

### UI in a plain browser

`npm run web` serves the frontend on sample data (`src/mock.ts` stands in for the Tauri commands; production builds leave it out). Useful for motion and copy changes without a Rust build. Add `?empty` to the address to see an empty library and the Getting started guide.

### Headless check without the window

```powershell
cd src-tauri
cargo run -p recall-core --bin probe -- index C:\Users\you\Downloads
cargo run -p recall-core --bin probe -- search "that paper about vents"
cargo run -p recall-core --bin probe -- status
```

The probe loads the model from `./models/`, which is why it runs from `src-tauri`. It uses the app's real data folder.

## Chats: sign in inside the app

`+ Add sources › Claude / ChatGPT / Gemini`, Settings › Chat apps › Connect, or the Getting started guide opens a `WebviewWindow` labelled `connect-<platform>` on the platform's own site, with `src-tauri/connect/reader.js` as its initialization script. Recall never sees the password: the person signs in to the site itself.

The script notices sign-in on its own, checking every 3 seconds for ten minutes, because some sign-ins (Claude's Google pop-up, a typed code) finish without a page load. Then the window hides, Recall comes forward and says what happens next, and the backup starts:

- **Quick backup** first: recent and starred chats. It runs again every fifteen minutes.
- **Full backup** of older chats, 50 at a time, only while the computer has been idle for five minutes or between 1 and 6 am, until every chat is in.

The reader asks the app which chats it already has and when each changed (`connect_known`), so only new or changed chats are fetched, and reports progress (`connect_progress`) for the line under each app in Settings. If a site asks Recall to slow down for longer than 15 seconds, the run stops and that app pauses for 30 minutes. Batches arrive through `connect_ingest` and the result through `connect_report`; the `connect` capability exposes only these four reader commands, to those sites only, and each command answers only its own platform's window. Chats from the page are checked before they are stored: the app sets the platform and builds each link itself, ids must be plain, and titles, messages and batches are capped. The window goes only to https pages and cannot download files. Disconnect deletes that app's own sign-in cookies and nothing else. Connection state lives in the `connections` setting; the schedule is at the top of `src-tauri/src/connect.rs`.

Every app command is listed in `build.rs` so Tauri generates `allow-<command>` permissions; `capabilities/default.json` grants the main window its commands, `capabilities/connect.json` grants the four reader commands to the sign-in windows.

## How it indexes

Every item carries two axes that never get folded into the embedding:

- **kind**: chat, pdf, document, spreadsheet, code, image, text
- **source**: claude, gemini, chatgpt, or the folder you added

On Windows a file is identified by a hash of its path, creation time and attributes, so a rename or move makes a new item and the next sweep drops the old one; elsewhere by device and inode. A file whose content hash has not changed is not read again; a changed one is re-chunked and re-embedded. Chats are identified by platform and conversation id.

Chunks keep a **locator** (`p.3`, `L44`, `turn 12`, `Sheet1`) so a card can show where a hit lives before you open it, and an **anchor** for the deep link on click. Chat and document chunks keep their markdown; the card renders headings, bold and lists.

Search is hybrid: 0.72 vector cosine + 0.20 FTS5 keyword rank + 0.08 recency, grouped per item so one long chat can't flood the list. Weights are constants at the top of `core/src/search.rs`. While the model is still loading, search is keyword-only; anything read in that time is read again once the model loads.

### Rules

Settings › What to skip is a set of switches, saved as they change and applied identically in the sweep and the watcher (`core/src/rules.rs`): program and build folders, code projects (any folder with `.git`, `package.json`, `Cargo.toml` and the like; on by default), pictures with no text (on by default), hidden files, installers and archives, video and audio, a size cap, folders picked to skip, file types to include, and following shortcuts. Each kind also has a hard size cap, whatever the setting (`extract::hard_cap`). The presets expand to concrete folder names and extensions in `rules.rs`. A line under the switches shows how many files the current settings keep and skip.

Each sweep also prunes: anything in a folder the rules now skip, anything deleted, and duplicates of the same file leave the index. A folder on an unplugged drive is left alone.

### Excluding one item

Hover a result, press Hide. The item leaves the index, its vectors with it, and is recorded in `excluded` by identity; the file is not touched. On Windows, renaming or moving the file gives it a new identity, so it comes back. Undo is on the toast, and Settings › Hidden items lists everything with Show again. The menu's second row opens What to skip with that file's folder added to the skip list.

### Indexing

Starts the moment a source is added. The first time, a notice explains that CPU use is high while it runs and returns to normal when it finishes; after that, a toast. Progress shows in the sidebar footer (`done / queued`, current file) and a toast marks completion. Search runs throughout. Chats go ahead of files in the queue. A PDF, Word or Excel file that crashes, exhausts or hangs its reader is remembered by content hash and skipped until it changes, so one bad file cannot stop indexing or crash Recall on every start; Word and Excel files that would unpack past 1 GB are refused outright.

Closing the window keeps Recall running in the tray so folders and chats stay current; Quit is on the tray icon. Opening Recall again brings the running copy forward instead of starting a second one.

## Model

`src-tauri/models/model.json` names the model, its dimension, pooling (`cls` or `mean`) and query prefix; the engine reads it at boot and opens `vectors-<dim>.usearch`. `npm run model` fetches bge-small-en-v1.5 from a fixed revision and checks each file's SHA-256 (`scripts/fetch-model.mjs`); a different model needs a new revision, new hashes, and Settings › Storage › Rebuild index.

## Network

Recall itself sends nothing anywhere and listens on no port. The sign-in windows talk to the chat sites the person connected, as a browser would. Fonts are bundled. Feedback, when a build has an endpoint, sends only on Send and carries only the text typed plus the app version and OS name (`core/src/feedback.rs`). No identifiers, no usage data, no telemetry from ONNX Runtime (pyke builds have it off).

## What the window may touch

The main window holds no file permissions of its own. Thumbnails and quick view read files through the asset protocol, whose scope starts empty and holds only the folders turned on in Settings. Open and Show in folder name an item by id (`src-tauri/src/open.rs`); the app opens it only if it is a supported file inside a chosen folder, or a chat link on claude.ai, chatgpt.com or gemini.google.com. Code and script files open with their Edit action (Notepad unless the person picked an editor), never to run.

## Motion

One curve (`--ease`), one tempo band (140–260 ms), defined once at the top of `src/style.css`. Cards rise with a 30 ms stagger on a new query only; same-query refreshes stay still. Hits expand in place. Menus, dropdowns and dialogs fade with a small pop; dialogs use `@starting-style` so the close fades too. Everything is off under `prefers-reduced-motion`.

## Data

`%LOCALAPPDATA%\Recall\`
- `recall.sqlite` — items, chunks, FTS index, sources, rules, exclusions, settings (`secure_delete` on, so deleted text is overwritten)
- `vectors-384.usearch` — HNSW index, keyed by chunk id
- `recall.log`: what Recall did, for troubleshooting; starts over past 2 MB

Settings › Storage shows sizes; Rebuild index drops items, chunks and vectors and re-sweeps, keeping sources, rules and exclusions.

## Not done yet

- Importing a chat export file (a ZIP downloaded from a chat app) is not built.
- The installer is not code-signed, so Windows shows a SmartScreen warning ("More info", then "Run anyway").
- Text inside pictures is not read; a picture is found by its name.
- The index on disk is not encrypted; it is as private as the Windows account it lives in.
