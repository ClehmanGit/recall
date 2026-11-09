# Changelog

## 2.1.3 (November 8, 2026)

- "Continue with Google" and "Continue with Apple" on Claude's sign-in page work. They open a pop-up, which the sign-in window used to block without a word. Pop-ups open only from a sign-in window you can see, and only for Google, Apple or Microsoft sign-in pages or the chat site itself.
- **Sync all now** in Settings › Chat apps, and on the notice after signing in, syncs every chat right away instead of only while the computer is idle.
- "Back up" is now "Sync" everywhere in the app.
- The sidebar's "Sources" heading is now "Filter by source", which is what clicking one does.

## 2.1.2 (November 8, 2026)

- Uninstalling Recall now removes everything it stored: the search index, settings, log and chat sign-ins. Your own files are not touched. Updating keeps everything, as before.
- The picture rule in **Settings › What to skip** now does what it says: it skips pictures named only by a camera or a number, such as `IMG_0412.jpg` or `Screenshot 2026-01-02.png`. Before, it skipped every picture with a name shorter than 20 characters, so pictures like `floor_plan.png` were left out. They are now found by name.
- Recall is now open source under the MIT license.

## 2.1.1 (November 8, 2026)

### Search

- Search by meaning works. In 2.1.0 the model failed on every passage, so only exact words matched. After the update, Recall embeds your existing library in the background, which can take a few hours on a large library with high CPU use. Search works throughout and improves as it goes.
- File names and chat titles are searched, so "kitchen quote" finds *kitchen_quote_2026.pdf*. Each passage is also embedded with its item's name, so a file can be found by what it is called even when its text never says it.
- Better ranking: a weak match by meaning no longer outranks an exact word or name match.
- The vector index is half the size on disk.

### Security and privacy

- Recall's window can open only files inside the folders you chose, and only chat links on Claude, ChatGPT and Gemini. Code and script files open in an editor and are never run.
- Thumbnails and previews can read only the folders you chose. Before, they could read any file.
- Each chat sign-in window can act only for its own app, can't download files, and can't leave https pages. Chats it sends are checked before they are stored.
- Disconnect signs you out of that one app. Before, it signed you out of all three, and did nothing after a restart.
- Hiding an item now also deletes its search data, and deleted text is overwritten on disk.
- Recall no longer listens on a local network port. It was left over from a browser extension that never shipped.
- Release builds are pinned to exact versions of every build step and of the model, and each release includes `SHA256SUMS.txt`.

### Reliability

- A damaged or hostile PDF, Word or Excel file can no longer crash Recall or stop indexing. Recall remembers it and skips it until it changes.
- Unchanged files are no longer read again on every start.
- A dropped connection during a chat backup no longer shows the app as signed out.
- Rebuild index now clears the search data too.

### Removed

- The "Read text inside images" switch, which did nothing.
- The sample library that was bundled into the installer.

## 2.1.0 (November 7, 2026)

First public release.

- Search across folders and Claude, ChatGPT and Gemini chats.
- Sign in to each chat app inside Recall. Recent and starred chats are copied first, older chats while the computer is idle.
- A Getting started guide on first launch.
- Windows installer, per-user, no administrator prompt.
