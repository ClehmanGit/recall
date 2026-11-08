# Development

## Layout

```
core/          recall-core: reading files, rules, embedding, vector index, search. No UI.
  src/bin/     probe: runs the engine without the window
src-tauri/     the Tauri app: window, commands, sign-in windows, installer
  connect/     reader.js, injected into the chat sign-in windows
src/           front end: plain TypeScript and Vite
  views/       search, gallery, timeline, getting started, settings, quick view
site/          the download page (GitHub Pages)
scripts/       fetch-model.mjs, update-installed.ps1
docs/          these notes
```

## Setup

Once:

1. Rust from https://rustup.rs with the MSVC toolchain. Accept the Visual Studio Build Tools prompt ("Desktop development with C++").
2. Node 20 or newer from https://nodejs.org.
3. WebView2, which Windows 10 and 11 already include.

Then:

```powershell
npm install
npm run model   # downloads bge-small-en-v1.5 (int8, about 33 MB) into src-tauri/models/
npm run dev     # the first run compiles the Rust side for several minutes, then opens the window
```

`npm run model` fetches a pinned revision and checks each file's SHA-256 before keeping it.

## Commands

| Command | What it does |
| --- | --- |
| `npm run dev` | Runs the app with hot reload for the front end |
| `npm run build` | Builds the installer into `target/release/bundle/nsis/`. The model is bundled, so nothing is downloaded on first launch. |
| `npm run web` | Serves the front end alone on sample data (`src/mock.ts`) at http://localhost:1420. Add `?empty` to see a new, empty library. Production builds leave the sample data out. |
| `npm run web:build` | Type-checks and builds the front end |
| `cargo test -p recall-core` | Runs the engine tests. One test loads the real model and is skipped until `npm run model` has run. |

### Running the engine without the window

```powershell
cd src-tauri
cargo run -p recall-core --bin probe -- index C:\Users\you\Downloads
cargo run -p recall-core --bin probe -- search "that paper about vents"
cargo run -p recall-core --bin probe -- status
```

The probe loads the model from `./models/`, which is why it runs from `src-tauri`. It uses the app's real data folder.

### Updating your own installed copy

`scripts/update-installed.ps1` builds the app, closes the running Recall, replaces `recall.exe` and starts it again. The library and settings are kept.

## Releasing

Releases are built by GitHub Actions (`.github/workflows/release.yml`), not on a developer's machine.

1. Bump the version in `package.json`, `core/Cargo.toml`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, and add an entry to `CHANGELOG.md`.
2. Update the version and installer size in `site/index.html`.
3. Commit, then tag and push:

   ```powershell
   git tag v2.1.2
   git push origin main v2.1.2
   ```

About fifteen minutes later the release has `Recall_<version>_x64-setup.exe`, the same file as `Recall-Setup.exe` (the fixed name the website links to), and `SHA256SUMS.txt`.

Every action in the workflows is pinned to a full commit SHA, and the model to a revision and SHA-256, so nothing upstream can change what goes into the installer without a commit here. Dependabot proposes updates as pull requests.

**Feedback box.** A build includes the feedback box only if the `RECALL_FEEDBACK_URL` repository secret holds an https endpoint. Without it the box is hidden and the app makes no outbound requests. Official releases currently have none.

**Code signing** is not set up. To remove the SmartScreen warning, get a signing identity (Microsoft's Trusted Signing is the low-cost route), set `bundle.windows.signCommand` in `src-tauri/tauri.conf.json` to its signing command, and store the credentials as repository secrets.

## The download page

`site/` is a single static page: what Recall does, the download button and four privacy points. It loads nothing from other sites; fonts, images and scripts are all in `site/`, and its Content-Security-Policy blocks everything else. It sets no cookies and has no analytics.

The download button points at `releases/latest/download/Recall-Setup.exe`, so a new release needs no change to the link. `.github/workflows/site.yml` fills in the repository name and publishes `site/` to GitHub Pages whenever a push to `main` changes it.

To preview it locally:

```powershell
python -m http.server 4321 --directory site
```
