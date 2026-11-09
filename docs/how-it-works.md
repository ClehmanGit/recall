# How Recall works

## Indexing

Every item has a **kind** (chat, pdf, document, spreadsheet, code, image, text) and a **source** (claude, chatgpt, gemini, or a folder you added). Both are stored as plain fields and used as filters; neither is folded into the embedding.

Each item is split into **chunks** of text. A chunk keeps a **locator** that shows where it came from (`p.3`, `L44`, `turn 12`, `Sheet1`) and an **anchor** used to open the item at that spot. Chat and document chunks keep their Markdown, so result cards can show headings, bold text and lists.

**Identity.** On Windows a file is identified by a hash of its path, creation time and attributes. Renaming or moving a file therefore makes a new item, and the next sweep drops the old one. On other systems a file is identified by device and inode. Chats are identified by platform and conversation id.

**Changes.** A file whose content hash has not changed is not read again. A changed file is split and embedded again. Each sweep also removes deleted files, duplicates, and anything the current rules now skip. A folder on an unplugged drive is left alone until it comes back.

**Queue.** Indexing starts as soon as a folder is added and runs on one background worker. Chats go ahead of files. Search keeps working throughout.

**Damaged files.** A PDF, Word or Excel file that crashes or hangs its reader is remembered by content hash and skipped until it changes, so one bad file cannot stop indexing or crash Recall on every start. Word and Excel files that would unpack to more than 1 GB are refused, and each kind has a hard size cap whatever the settings say (`extract::hard_cap`).

### Rules

**Settings › What to skip** is a set of switches applied the same way by the sweep and the file watcher (`core/src/rules.rs`): program and build folders, code projects (any folder with `.git`, `package.json`, `Cargo.toml` and similar), pictures named only by a camera or a number (`IMG_0412.jpg`; see `rules::camera_name`), hidden files, installers and archives, video and audio, a size limit, folders picked to skip, file types to include, and whether to follow shortcuts. A line under the switches shows how many files the current settings keep and skip.

### Hiding one item

**Hide** removes the item, its chunks and its vectors from the index and records its identity in the `excluded` table. The file itself is not touched. Because identity on Windows includes the path, renaming or moving a hidden file brings it back. **Settings › Hidden items** lists everything hidden.

## Search

Search is hybrid. For each query Recall combines:

| Signal | Weight |
| --- | --- |
| Vector similarity between the query and each chunk | 0.72 |
| SQLite FTS5 keyword rank | 0.20 |
| Recency | 0.08 |
| The item's name containing the query's words | up to 0.6 extra |

Results are grouped per item, so one long chat can't fill the list. Vector similarity is rescaled to the model's own range (0.45 counts as no match, 0.80 as a strong one) so that background noise can't outweigh a keyword or name match. Each chunk is embedded together with its item's name, so a file or chat can be found by what it is called even when its text never says it. The weights are constants at the top of `core/src/search.rs`.

The model is [bge-small-en-v1.5](https://huggingface.co/BAAI/bge-small-en-v1.5), quantized to int8 and run with ONNX Runtime on the CPU. `src-tauri/models/model.json` names the model, its dimension, pooling and query prefix. Vectors are stored at half precision in a [USearch](https://github.com/unum-cloud/usearch) HNSW index keyed by chunk id.

While the model loads, search is keyword-only. Once it has loaded, a background backfill embeds any stored chunk that has no vector yet, a slice at a time behind other work.

## Chat sync

Choosing Claude, ChatGPT or Gemini opens a separate window labelled `connect-<platform>` on that app's own website, with `src-tauri/connect/reader.js` injected. The person signs in on the site itself; Recall never handles the password.

The script checks every 3 seconds, for up to ten minutes, whether sign-in has finished, because some sign-ins (a Google pop-up, a typed code) finish without loading a new page. Then the window hides and the sync starts, using the site's own session:

- **Quick sync:** recent and starred chats, on sign-in and then every 15 minutes.
- **Full sync:** older chats, 50 at a time, only after five minutes without keyboard or mouse input or between 1 and 6 am, until every chat is in. **Sync all now** in Settings runs full syncs back to back instead, until every chat is in.

The script first asks which chats Recall already has and when each last changed (`connect_known`), so only new or changed chats are fetched. It reports progress for the line under each app in Settings (`connect_progress`), sends batches (`connect_ingest`) and reports the result (`connect_report`). If a site asks Recall to wait longer than 15 seconds, the run stops and that app pauses for 30 minutes. The schedule is at the top of `src-tauri/src/connect.rs`.

## Security model

### What the main window may do

- **No direct file or shell access.** The main window has no opener or dialog permissions. Every command it can call is listed in `src-tauri/build.rs` and granted in `src-tauri/capabilities/default.json`.
- **Opening items.** Open and Show in folder name an item by its id (`src-tauri/src/open.rs`). The app opens it only if it is a supported file inside an enabled folder, or a chat link on `claude.ai`, `chatgpt.com` or `gemini.google.com`. Code and script files open with their Edit action (Notepad unless another editor is set), never run.
- **Previews.** Thumbnails and quick view read files through Tauri's asset protocol. Its scope starts empty and holds only the folders turned on in Settings.
- **Adding folders.** A folder can be added only after the person picks it in the system folder dialog.
- **Content Security Policy.** The window loads nothing from outside the app: no remote scripts, frames, objects or forms.

### What a sign-in window may do

- It can call only the four reader commands (`src-tauri/capabilities/connect.json`), only from that app's own site, and each command answers only for its own platform. A Claude window can't read or write ChatGPT data.
- Everything the page sends is checked before it is stored. The app sets the platform and builds each chat link itself, chat ids must be plain identifiers, and titles, messages and batch sizes are capped.
- The window navigates only to https pages and can't download files.
- **Disconnect** deletes that app's own sign-in cookies and nothing else.

### Network

Recall sends nothing anywhere and listens on no port. The sign-in windows talk to the chat sites the person connected, the way a browser would. Fonts are bundled. ONNX Runtime's telemetry is off in the build Recall uses.

The only outbound request in the code is the optional feedback box (`core/src/feedback.rs`). It exists only in builds made with a feedback endpoint (official releases have none), sends only when the person clicks Send, and carries only the typed text, the app version and the OS name.

### Build

Release builds run on GitHub Actions with every action pinned to a commit SHA. The model is fetched from a fixed revision and checked against SHA-256 hashes. Each release publishes `SHA256SUMS.txt` for the installer.

## Data on disk

Everything lives in `%LOCALAPPDATA%\Recall`:

| File | Contents |
| --- | --- |
| `recall.sqlite` | Items, chunks, the full-text index, sources, rules, hidden items and settings. `secure_delete` is on, so deleted text is overwritten rather than left in free pages. |
| `vectors-384-f16.usearch` | The vector index |
| `recall.log` | What Recall did, for troubleshooting. Starts over past 2 MB. |

The sign-in windows keep their cookies in WebView2's profile under `%LOCALAPPDATA%\app.recall.desktop`.

**Settings › Storage** shows sizes. **Rebuild index** drops items, chunks and vectors and reads everything again, keeping sources, rules and hidden items.

**Uninstalling** removes both folders (`src-tauri/nsis/hooks.nsh`). The program is installed into `%LOCALAPPDATA%\Recall` by default too, and an update runs the old uninstaller first, so the hook does nothing during an update; only a real uninstall removes data.
