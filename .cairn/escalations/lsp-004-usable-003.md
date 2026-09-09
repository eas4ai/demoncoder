DECISION

Question:   May language servers use a filtered project copy and explicit external dependency access instead of broad live filesystem access?
Recommend:  Use the filtered copy for language servers only; keep ordinary Bash and native read access unchanged.
Because:    The new regression proves a running server reads a protected directory created after startup. A filtered view can exclude private files throughout its lifetime; restarting or watching leaves read races.
If wrong:   Projects using sibling source trees or unusual local tools may need explicit dependency access before navigation and diagnostics work. Copying and synchronizing project inputs also adds overhead.
Instead:    Preserve broad live access by adding filesystem mediation, with substantially more implementation and operating requirements. Do not accept the current protected-file exposure.

Reply: ok | instead | ask. If this isn't clear, ask me to explain it another way before you decide.

Concerns: LSP-004, USABLE-003
Status: open
Raised: 2026-09-09T15:50:31.367Z
Raised after: LSP-004=1 USABLE-003=unknown
Answer: ask Explain what the filtered project copy changes for my projects and why it is needed.
Answered: 2026-09-09T16:03:43.046Z
Answered after: LSP-004=1 USABLE-003=unknown
Answered order: 3
Reply: A language server is the background program that supplies definitions, types and compiler errors. The current implementation lets it see the live filesystem while hiding protected paths found at startup. Our test started the server, then created a new protected directory; the server could read it. My recommendation is to give the language server a separate filtered view containing project files and explicitly permitted toolchain and dependency files. DemonCoder would synchronize allowed changes into that view; private files would never enter it. Your edits would still affect the real project, and ordinary Bash and read tools would retain their existing access. The cost is additional copying and synchronization. A project importing a sibling directory such as ../shared might need that directory explicitly included before the server can analyze it. The alternative is to preserve broad live access and build a layer that checks filesystem access as it happens; that adds substantially more infrastructure. Approval authorizes the narrower language-server view and its dependency limitations, not a restriction on ordinary coding tools. Neither design is implemented yet. If this is not clear, ask me to explain it another way before you decide.
Replied: 2026-09-09T16:03:53.934Z
Answer: ask Should LSP respect .gitignore and debounce filesystem changes like fast-context, or does that conflate separate issues?
Answered: 2026-09-09T16:10:32.737Z
Answered after: LSP-004=1 USABLE-003=unknown
Answered order: 4
Reply: Ignore rules choose which files DemonCoder proactively indexes or sends change notifications for. Debouncing coalesces rapid changes to reduce repeated analysis. Both are useful, and fast-context implements watcher filtering and batching. Neither prevents a separate language-server process from opening a file directly through its live filesystem access. The failing canary test exercises that direct read. Git ignore rules also commonly exclude dependencies or generated files that language analysis needs, so they should guide background scanning rather than universally deny reads. Protected-file exclusions remain an independent access rule. The inspected fast-context watcher uses substring ignore matching and a nominal 500 ms debounce; its event loop checks flushing only upon receipt of an event, so a final small batch can remain pending without another event or an explicit flush. Reuse the pattern with proper Git ignore semantics and timer-driven flushing, not the implementation unchanged. The filtered-view decision remains open; this discussion authorizes no access-policy change.
Replied: 2026-09-09T16:10:32.863Z
Answer: ok
Answered: 2026-09-09T16:16:41.485Z
Answered after: LSP-004=1 USABLE-003=unknown
Answered order: 5
