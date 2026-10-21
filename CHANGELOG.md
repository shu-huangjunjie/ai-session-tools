# Changelog

## 0.1.3 (2026-10-21)

- Add independent work-folder pins through the folder header or its context menu. Pinned folders appear above ordinary groups, persist by source and normalized Windows path, and retain their header when all children are individually pinned. Schema 9 backs up and upgrades existing databases. Clearing the vault also clears folder pins.
- Add persistent conversation pins: use the row's pin button or right-click menu to pin/unpin. A dedicated group at the top of each source shows pinned search results once, newest pins first. Pins survive rescanning and restarts; the schema 8 migration backs up existing databases before adding the pin timestamp.
- Replace the Management Center with a header button to clear the local database. Preview all-source counts, require confirmation and back up before clearing conversations (including archived ones), messages and deletion records. Stay on the empty workspace afterwards so rescanning is a deliberate next action.
- Batch Markdown export uses the Explorer-style folder picker with an address bar, search and new-folder support. It starts in the last successful export directory, or the selected conversations' shared project folder on first use.
- Normalize the project path after Rust canonicalization before launching VS Code, so Explorer integration receives a regular Windows path.

## 0.1.2

- Run every backend command on a background thread. Scans, publishing, exports and file dialogs no longer freeze the window.
- Codex: read the thread indexes whose timestamps are stored as integers; previously every `state_5.sqlite` index failed to load. Exclude internal `guardian_review` threads. Recover threads whose rollout file is gone from `thread_history_N.sqlite`. Parse rollouts about twice as fast.
- Codex publishing: map selected conversations to their Codex thread IDs. Previously the IDs never matched, so a publish would have hidden every thread; it also failed outright because `PRAGMA busy_timeout` was rejected. Publishing now refuses when no selected conversation belongs to Codex, leaves subagent threads alone (and restores ones hidden by 0.1.1), and asks for confirmation in the app with the number of threads that will be shown and hidden.
- Search: switch the full-text index to the trigram tokenizer, so words inside Chinese sentences are found; one- and two-character terms fall back to a substring scan. Every term must match the title or a message. The database migrates to schema 7 on first start and is backed up before the migration.
- Claude: write new sessions into the project folder Claude Code uses (`G:\web\demo` → `G--web-demo`) together with a `custom-title` entry. Read titles from transcript `custom-title` entries, extract tool results given as lists, and skip injected `isMeta` messages. Restarting Claude after publishing is now an option that is off by default, because it ends every `claude.exe` process, including Claude Code sessions.
- Antigravity: store publish times in UTC (local time was labelled `+00:00`), percent-encode and decode workspace paths with non-ASCII characters, write summaries in one transaction, keep index rows that contain NULL columns, and start the newest installed client.
- Imports: keep ChatGPT conversations connected when tool calls or hidden system messages are skipped, prefer `conversations.json` over other JSON files in an export ZIP, and accept OpenAI content given as a list of parts. Choose files with a native dialog.
- Interface: Enter in the rename dialog saves instead of cancelling; renaming updates the open conversation; the batch selection is cleared when switching between Codex, Antigravity and Claude and after deletions; search waits for a pause in typing; Ctrl+K focuses search; the context menu opens at the cursor and closes on outside clicks; dialogs are readable in the light theme; thinking blocks are collapsible; conversations open at their latest message.
- Exports: file names are made valid for Windows (reserved names, control characters, length).
- Management: "Clear vault" backs up the database, then removes the conversations of the current source or of all sources without recording them as deleted, optionally clears earlier deletion records, and starts a new scan.
- Codex tab: list conversations the way the Codex sidebar does. "Listed in Codex" shows the projects from `.codex-global-state.json` in sidebar order with the threads assigned to them, then the chats outside any project; threads that are only in a project's folder, archived in Codex, or left over from removed projects go to a "Not listed in Codex" section that is collapsed by default, remembers its state and opens while searching. The Codex count reads "listed/total". The state is read again on every list reload and when the window regains focus. The scan dialog uses the same sections and preselects only new threads that Codex lists. The `codex-dev.db` index is labelled "Codex 线程缓存", since it caches every thread Codex has seen rather than the sidebar. Without project state (older Codex builds) the tab keeps grouping by folder.
- Startup errors are shown in a message box instead of the app closing silently; panics are appended with their message to `logs/crash.log`. A missing WebView2 runtime is reported with its download address.
- Link the C runtime statically, so the exe no longer needs the Visual C++ Redistributable; with the statically linked WebView2 loader the portable folder only contains the exe and its data. `build.ps1 -PortableDir <folder>` updates a portable folder in any location and no longer writes a separate launcher.
- MinGW builds (`x86_64-pc-windows-gnullvm` / `-gnu`, used when no Visual Studio Build Tools are installed) link the unwinder statically instead of requiring `libunwind.dll`. They load the WebView2 loader at run time, so `build.ps1` now copies `WebView2Loader.dll` next to the exe for these toolchains; without it the exe failed to start with "找不到 WebView2Loader.dll". `build.ps1` builds with the `stable-x86_64-pc-windows-msvc` toolchain whenever it and the Visual Studio C++ build tools are installed, even if another toolchain is the default, and then removes a leftover `WebView2Loader.dll` from the output folders.

## 0.1.1

- Back up Codex and Antigravity SQLite databases with SQLite's online backup API so committed WAL records are retained. Stop publishing if the backup fails.
- Preserve existing Claude JSONL transcripts and unknown title/history metadata. Back up affected files before publishing; write metadata through temporary files.
- Import all messages before resolving their parent links, so ChatGPT exports work regardless of mapping order. Apply imports in a transaction and reject cycles when loading a message path.
- Distinguish files without native conversation IDs by their content hash. Different files with the same name no longer merge; importing the same file again remains idempotent.
- Apply the archive checkbox to conversation browsing and search, and ignore stale list responses.
- Select another development port when the default is occupied, without stopping another process or rewriting the checked-in Tauri configuration.
- Update Vite, esbuild, and the test dependencies; the checked dependency audit reports no vulnerabilities.
- Add isolated regression tests and keep three tests that require installed-client histories out of the default test run.

If a ChatGPT export was imported by an older version with missing parent links, import that original export again to repair the links. Existing conversation data is retained when updating the local application binaries.

The Windows ZIP contains a clean portable application with an empty `data/` directory. The source ZIP contains code, assets, scripts, tests, and dependency lockfiles; it excludes local conversation databases, backups, dependencies, and compiled output.
