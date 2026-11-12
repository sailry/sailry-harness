# Changelog

Release entries supply the corresponding GitHub Release notes. A version is
released only after its entry is here.

## [0.1.6](https://github.com/sailry/sailry-harness/releases/tag/v0.1.6) — Unreleased

This is the first release built from the Go rewrite, which replaces the Rust
implementation in this repository. Desktop for macOS and Windows, the Host for
Linux and macOS, and Mobile keep their download names and the `~/.sailry`
profile. The Go Node stores sessions and settings in new SQLite files beside the
earlier release's database, which it leaves untouched, so sessions and settings
from 0.1.5 do not appear.

- Fill the conversation pane with the transcript under a docked composer, with turns fading out above it
- Fold the queue tray under a header that counts the queued messages
- Open a conversation's links and files in the side panel, or in the system's app with Command or Control
- Attach files through the same file picker that chooses project folders
- Set the message text size a pixel at a time and draw reply diagrams natively
- Retry a failed model request or summary every 60 seconds, nine times, and fail a request that stays silent for 600 seconds
- Show the connection banner only for a real interruption and reconnect without a Retry button
- Keep the answer and complete tool calls of a reply cut at the output limit
- Add GitHub Copilot sign-in by device code
- Bundle WorkBuddy with a warning before sign-in, free models only by default and opt-in daily check-in
- Use an installed command-line agent as a model over the Agent Client Protocol, with the Antigravity package in the Market
- Start a sign-in's advanced settings on the values in use
- Ask an Azure OpenAI channel for its API version and name its models by deployment
- Fold the model picker's groups and mark chosen rows with a neutral fill
- Give goals a state badge, Pause, Resume and End, and turn, token and time limits
- Draw the Git history as a commit graph, add a Unified and Split choice to diffs and show conflicted paths
- Add a More menu, Unarchive and session reordering to the project page and list its records
- Rebuild the host page as a monitor with versions in its header, chart tooltips, a GPU card and a process table
- List notifications in one card with each notice's message
- Show Usage as compact tables and Market cards without their source
- Close Settings from the rail's Settings button
- Add Demo mode under Settings, Experimental, which shows sample conversations and sends nothing
- Back up while the Node runs, with retention and the last backup's record
- Keep SSH connections alive and prefer the trusted host key's algorithm
- Fire due schedules after the computer wakes

## [0.1.5](https://github.com/sailry/sailry-harness/releases/tag/v0.1.5) — 2026-11-08

- Replace Goose execution with Sailry's Agent loop and Rig provider integration
- Share MCP connections while isolating session cancellation, approvals and failures
- Keep device pairing reliable during concurrent storage writes
- Improve tool execution, Git pagination, file search and failure reporting
- Add pinned sessions and terminals, project and branch menus, and model scores
- Refine model controls, connector status, keyboard shortcuts and desktop updates
- Preserve mobile model features and refresh views when returning to the app
- Simplify browser element references and correct Windows browser layering and tab contrast
- Publish macOS and Windows Desktop, macOS and Linux Host, Android and iOS packages

Linux Desktop is not supported in this release.

## [0.1.4](https://github.com/sailry/sailry-harness/releases/tag/v0.1.4) — 2026-11-05

- Relicense Sailry under Apache-2.0 ([baacb30](https://github.com/sailry/sailry-harness/commit/baacb301f030fc26b45a91acdae7b8396a509eb1))
- Keep the install progress ring circular ([612bf4b](https://github.com/sailry/sailry-harness/commit/612bf4b33d012155cc52d44192309625a5085125))
- Name provider picker collection types ([8773406](https://github.com/sailry/sailry-harness/commit/87734064fb70fc9999e08ad67829cc6395cc2430))
- Compile macOS tests once and shard their runs ([96111c9](https://github.com/sailry/sailry-harness/commit/96111c96dc1d9dd58bf138921e80d3d16b3a8496))
- Include runtime parsers in shared test archives ([6dc39b5](https://github.com/sailry/sailry-harness/commit/6dc39b54bd9eca0f6d1e13abeaf8c2a1c8e07492))
- Keep automatic session read acknowledgements silent ([ad24107](https://github.com/sailry/sailry-harness/commit/ad241078e25b856cebdf4e322a63f79c1b56f9ae))
- Retry conflicting automatic read acknowledgements ([2ad8e3b](https://github.com/sailry/sailry-harness/commit/2ad8e3b4143f2414867594de30bf26af9aac6416))
- Require shared test shards for product readiness ([bc2cb20](https://github.com/sailry/sailry-harness/commit/bc2cb20afca6161311da974a244e7e8d0cd0b7b4))
- Bound file previews with incremental size accounting ([fc69e5c](https://github.com/sailry/sailry-harness/commit/fc69e5c18a184585151066f4eabefd345a428623))
- Settle cancelled delegated waiters before releasing ownership ([7891be6](https://github.com/sailry/sailry-harness/commit/7891be645b2a7009ceaeb78525622c4301d65570))
- Reuse verified Checks results for unchanged product inputs ([3431eaf](https://github.com/sailry/sailry-harness/commit/3431eaf77b0aea6bcda6edea41e362d08e016f06))
- Allow interrupted conversations to reuse attachments ([cafd54d](https://github.com/sailry/sailry-harness/commit/cafd54dfde3421ac0374d78ac77df2779174be3f))
- Prepare Sailry 0.1.4 ([45b1702](https://github.com/sailry/sailry-harness/commit/45b17024543a2caafba8651cab950f79fbd6df81))

[Full changelog](https://github.com/sailry/sailry-harness/compare/v0.1.3...v0.1.4)

## [0.1.3](https://github.com/sailry/sailry-harness/releases/tag/v0.1.3) — 2026-11-04

- Release browser resources when closing sidebar ([96b975d](https://github.com/sailry/sailry-harness/commit/96b975dcc6ef77441f97228c26f4fe6f7f70684f))
- Publish recent conversation history before statistics ([65fe5b4](https://github.com/sailry/sailry-harness/commit/65fe5b45f6ac4305af90ae8c6e29961556a197b3))
- Report controller presence from active subscriptions ([07d7b40](https://github.com/sailry/sailry-harness/commit/07d7b4076d29d55ede10ca236e8de95e17c70f9a))
- Keep steering responsive and merge compatible followups ([7a7e0bf](https://github.com/sailry/sailry-harness/commit/7a7e0bf805cb37e1b6c72fc65ce6fddfdbd779e8))
- Decide Link peer compatibility by protocol number ([e306c3b](https://github.com/sailry/sailry-harness/commit/e306c3bfa1e5826783d80fee3b3090e4096cbaaf))
- Time run startup stages and connect MCP servers concurrently ([2045f7a](https://github.com/sailry/sailry-harness/commit/2045f7a4bdf53de032b7f2f7f172998021706ebb))
- Measure model latency as time to first token ([6a03774](https://github.com/sailry/sailry-harness/commit/6a037747452fbeeae5b2b975f28b1a8eea2b77a7))
- Update statistics plugin minimum version ([98d452f](https://github.com/sailry/sailry-harness/commit/98d452fa0cd2d2a8f62c7e9e437c76e4f10c4e30))
- Return edited regions and leaner tool results to the model ([4a866c7](https://github.com/sailry/sailry-harness/commit/4a866c7c708b6144da33d05da458cb020c868e4b))
- Update plugins for files 0.1.22 ([e1c4d1c](https://github.com/sailry/sailry-harness/commit/e1c4d1cc4f6a78d52d330f86337b88f3c8c19399))
- Qualify guidance commands in request persistence ([651a534](https://github.com/sailry/sailry-harness/commit/651a534e66a2efe9824a7b67a4d9db71aa9d5b4f))
- Cut code index per-query reconcile overhead without skipping validation ([efdd8ad](https://github.com/sailry/sailry-harness/commit/efdd8adc229c5d5bcc452cadc827338cc069339a))
- Fix guidance merge and stop edge cases from review ([42ef95f](https://github.com/sailry/sailry-harness/commit/42ef95f66f64dbcbf04d59929ea80022999e81b4))
- Fix foreground delegation, guidance and latency review findings ([5cf7884](https://github.com/sailry/sailry-harness/commit/5cf78840d48b2e09883e9e03944a14d97befc8ba))
- Fix remaining guidance, latency, web and host update review findings ([90a1d4a](https://github.com/sailry/sailry-harness/commit/90a1d4a82689a7f2a862327b3290fba2d6c67c4e))
- Compare recovered attempt snapshot against the recorded attempt ([b881167](https://github.com/sailry/sailry-harness/commit/b8811677299c72130a02e9266d2bed3aef3fa56f))
- Keep explicitly stopped input as the stopped turn instead of requeueing it ([1d7b58d](https://github.com/sailry/sailry-harness/commit/1d7b58d514c6fbd27e30b5606e77805f11113b90))
- Align node runtime tests with bounded output, summary and statistics contracts ([c3be636](https://github.com/sailry/sailry-harness/commit/c3be6364689613722ee107a19dafb3dfdf06f028))
- Show user-stopped tool rows in their normal style ([9cc9ea0](https://github.com/sailry/sailry-harness/commit/9cc9ea0fa57ea05f68dfd0975af137f23cc8d70e))
- Make running input a single send action without immediate guidance ([cc623d6](https://github.com/sailry/sailry-harness/commit/cc623d6190fd4795b2d2f49c48f84a2cb252effb))
- Show turns in execution order and keep stopped input in place ([7ff3b0f](https://github.com/sailry/sailry-harness/commit/7ff3b0fdaa99d3e0300bb2b903d0677ece4a8ce8))
- Move long foreground commands to the background when guidance is pending ([6b24626](https://github.com/sailry/sailry-harness/commit/6b2462618730e13714a4fc3b3a78eddc65cd6950))
- Show live output and hold completion for promoted commands ([a4e825a](https://github.com/sailry/sailry-harness/commit/a4e825a482460d1e6dd6441539739104a7a540c6))
- Expect stopped input in history rather than a projected copy ([f101e27](https://github.com/sailry/sailry-harness/commit/f101e276b6ba9c3446644938c160eed7304143c4))
- Consolidate protocol and content behavior matrices ([1661545](https://github.com/sailry/sailry-harness/commit/16615459cf91440bc7ad16bd80431062a4e306ca))
- Consolidate Client and Flutter behavior scenarios ([b3d4e48](https://github.com/sailry/sailry-harness/commit/b3d4e4863d02e5a855e87e9a4c07785fb458c5be))
- Consolidate Desktop behavior scenarios ([f8eed73](https://github.com/sailry/sailry-harness/commit/f8eed730050a6fcb2f7b9a66652b4edf03c7038a))
- Bound guidance admission stack on Tokio workers ([e86db0e](https://github.com/sailry/sailry-harness/commit/e86db0e84d467be89cbbcb4ae7d93fa91cc32014))
- Consolidate Node pure behavior matrices ([fabf390](https://github.com/sailry/sailry-harness/commit/fabf3903daac98747e113cab756878aea5b5b0c9))
- Consolidate Store behavior scenarios ([419e9d9](https://github.com/sailry/sailry-harness/commit/419e9d907ce20696994eb58595940c4996f876cd))
- Consolidate Link relay rollback scenarios ([76c3882](https://github.com/sailry/sailry-harness/commit/76c3882d8c09334a1291a6cbf7ad13cdfe4c9d2f))
- Consolidate Agent integration behavior scenarios ([eae0ef3](https://github.com/sailry/sailry-harness/commit/eae0ef313c4d0f410eb39d7aa0dfea8b73d70692))
- Check model progress before the installation registry exists ([a33f30b](https://github.com/sailry/sailry-harness/commit/a33f30b10bafa4033486d6d3902c1f8052ac24d6))
- Label consolidated scenario cases and check media revision steps ([d32d15e](https://github.com/sailry/sailry-harness/commit/d32d15e245c7345be6037fc8166d4d30328b07d2))
- Restore Tab to queue input after the current turn ([30b8162](https://github.com/sailry/sailry-harness/commit/30b8162fab76484c3e2ae1391b72f209b2daf211))
- Update plugins for code context revision conflicts ([065d98b](https://github.com/sailry/sailry-harness/commit/065d98bac9981238c58a93684f0197ea0846f254))
- Mark joining queue input with the loader and keep background notices in app ([c89ef14](https://github.com/sailry/sailry-harness/commit/c89ef14288b447cd2f506d9c07d0dc2e4569018d))
- Show session title as an ordinary tool row and label TTFT directly ([259c444](https://github.com/sailry/sailry-harness/commit/259c44472316d55be2bb4569f7bbe16b2f6a3403))
- Refresh matched code index files before returning search results ([947a586](https://github.com/sailry/sailry-harness/commit/947a5868fcb595ea0c81003f9b51133cc9a0bf44))
- Drop the package column from the composer command menu ([27f4e2f](https://github.com/sailry/sailry-harness/commit/27f4e2f59955cba382f31089191a2dcbb7e9d68e))
- Add a session order setting for created or updated time ([202c7ee](https://github.com/sailry/sailry-harness/commit/202c7ee29a68376ea45797d7066faaf4342ac5f7))
- Dim sidebar session ages below their titles ([0c96944](https://github.com/sailry/sailry-harness/commit/0c969448455f802c23d6a1e02c361a2b2b09029e))
- Prepare v0.1.3 release versions and plugin catalog ([d676a4f](https://github.com/sailry/sailry-harness/commit/d676a4fdf5d194df47e97442b5d647443f55f6d1))
- Watch only delegated sessions waiting on approval ([ce67155](https://github.com/sailry/sailry-harness/commit/ce67155b802754c7c58ed06cca26383f6f70b969))
- Resolve strict Rust lint findings for release ([5cd7db8](https://github.com/sailry/sailry-harness/commit/5cd7db8bd0c17b933f4f6fe7d8e2ab384af09ad9))
- Keep Host and queue presentation lint clean ([6433b9c](https://github.com/sailry/sailry-harness/commit/6433b9cb0c2fdba8547bbeda022044c78b9bc5c9))
- Group queue row presentation options ([3f2654a](https://github.com/sailry/sailry-harness/commit/3f2654adcb1d7e2cce53703e6b67c6f457d4db39))
- Remove redundant copies in document fixture cases ([7355b06](https://github.com/sailry/sailry-harness/commit/7355b0665fc864b4ba77ed0e5c95bdf068a83883))
- Align release fixtures with current behavior ([a0647d3](https://github.com/sailry/sailry-harness/commit/a0647d338eaf283713da2fa122c489a571a9d7d8))

[Full changelog](https://github.com/sailry/sailry-harness/compare/v0.1.2...v0.1.3)

## [0.1.2](https://github.com/sailry/sailry-harness/releases/tag/v0.1.2) — 2026-11-03

- Fix image alignment and HTTP browsing for 0.1.2 ([134cae2](https://github.com/sailry/sailry-harness/commit/134cae25a82a64650ca3220c8a19792eed512c88))

[Full changelog](https://github.com/sailry/sailry-harness/compare/v0.1.1...v0.1.2)

## [0.1.1](https://github.com/sailry/sailry-harness/releases/tag/v0.1.1) — 2026-11-03

Upgrade Desktop, Mobile and Host together. Terminal input uses a new batch format
and is not compatible with version 0.1.0.

- Batch terminal input across local and remote controllers and limit pointer backlogs
- Restore notification history without replaying old alerts
- Use Enter to guide an active task and Tab to queue input
- Keep original tool history without separate summary requests for older tool calls
- Show recent session activity and the latest lines of command output
- Use fixed Mobile download filenames and recover Desktop packages after symbol upload failures

## [0.1.0](https://github.com/sailry/sailry-harness/releases/tag/v0.1.0) — 2026-11-02

- Update statistics to 0.1.24 with first-step bars, speed fallback and grouped tool names ([7128834](https://github.com/sailry/sailry-harness/commit/71288348498a3cc298fada59418f3e8088a27261))
- Update files to 0.1.18 without the written result notice ([fd4ae2d](https://github.com/sailry/sailry-harness/commit/fd4ae2d49e37af5cacd39744534a656bdd62b95d))
- Reselect tool details when a hidden conversation pane is shown again ([9a92c0d](https://github.com/sailry/sailry-harness/commit/9a92c0dbd2ce2d1797ce18069d539ecdddc8a2ef))
- Show file writes and edits as line-numbered checkpoint diffs ([adc14db](https://github.com/sailry/sailry-harness/commit/adc14db6c777a18acbb7433776446348e3c5246b))
- Show background results that start automatic turns ([1988263](https://github.com/sailry/sailry-harness/commit/1988263319856b7b6856f7e56b9cf5ae154fa300))
- Tidy background command output and show elapsed time ([e88e0a8](https://github.com/sailry/sailry-harness/commit/e88e0a82d9c4e49afeb7619a968e5fd2694e719b))
- Skip conflict toasts for automatic session read acknowledgements ([335c0c9](https://github.com/sailry/sailry-harness/commit/335c0c9be9b7c04af77477c0ff0b4a3a7dbcb11e))
- Fix visible history search and current Mobile provider contracts ([725fd04](https://github.com/sailry/sailry-harness/commit/725fd04996112d4b7cc3a2c77dd53d2a59e2211c))
- Update delegation to 0.1.29 for the subagent delete icon ([8ab3501](https://github.com/sailry/sailry-harness/commit/8ab3501ca23426f9ea98061c1bacb155c819aa73))
- Stabilize terminal exit and registered worktree fixtures ([0a68667](https://github.com/sailry/sailry-harness/commit/0a68667a059fb14bf034294f5fce7d382ed18f4f))
- Speed up the conversation activity popover and fix header popover focus ([3e3ad9c](https://github.com/sailry/sailry-harness/commit/3e3ad9c59b2df43101915d1ee3f69d51281e77fb))
- Keep conversations live through visibility changes and lazy turn recovery ([fb715bf](https://github.com/sailry/sailry-harness/commit/fb715bfb783856caae11f77b16ce10a17903ba04))
- Render visuals and HTML in the document editor ([cf950f0](https://github.com/sailry/sailry-harness/commit/cf950f05aa6c25da2e80e5185177239af8de56e5))
- Add Markdown formatting toolbar, link editing and responsive files toolbar ([9b0cbd7](https://github.com/sailry/sailry-harness/commit/9b0cbd7356291a5ab65fa2817448e940b5fa6afb))
- Reuse session statistics for queue commands and report missing search scopes neutrally ([44632bf](https://github.com/sailry/sailry-harness/commit/44632bfb2f4327395c2ca08283626753a0f64216))
- Unify editor formatting and reuse document visuals ([889c75a](https://github.com/sailry/sailry-harness/commit/889c75a28fe040fff901307d29676bc5394824eb))
- Bound conversation reads and defer derived presentation work ([53681be](https://github.com/sailry/sailry-harness/commit/53681bedbf6e4daf9305373c0e5a7f6ac6261778))
- Align Claude OAuth requests and refresh handling with OMP ([8e4b338](https://github.com/sailry/sailry-harness/commit/8e4b33852dcec53ee0cb33058d6edf7fab804ec4))
- Pin GDK to the Sailry fork with tested provider fixes ([bb6fe98](https://github.com/sailry/sailry-harness/commit/bb6fe98dc1415f12eebfbf64882fc07db788b09d))
- Keep request rejections visible without reporting private details ([b9e9b32](https://github.com/sailry/sailry-harness/commit/b9e9b32927a979a33f2e4e97f1602af6af9cd72d))
- Compare captured execution settings when joining guidance ([8c9e452](https://github.com/sailry/sailry-harness/commit/8c9e452b7003e33baf9075b0fad7cc0bf8e81026))
- Destroy the conversation side panel when it is closed ([e774afd](https://github.com/sailry/sailry-harness/commit/e774afd4618c03915c83d21580534d742bc26a19))
- Report filesystem access failures as partial search results ([cddc964](https://github.com/sailry/sailry-harness/commit/cddc9643b8a207d33d55e10e7bd9045fc5e08d16))
- Classify expected tool and transport failures as warnings ([7799086](https://github.com/sailry/sailry-harness/commit/7799086a5f8be9f8a6a74ed7ecab4122b9f5c49d))
- Preserve and upload symbols for local Desktop distribution builds ([b26b414](https://github.com/sailry/sailry-harness/commit/b26b414e80c6fbb75b1ca0d304f002b998ccc5dd))
- Scope code search issues and rank exact definitions first ([efe8f47](https://github.com/sailry/sailry-harness/commit/efe8f47ce957ede7ce5e4714b3211e5f3e5ad00f))
- Update plugins for code 0.1.11 ([8bd0efe](https://github.com/sailry/sailry-harness/commit/8bd0efe9b4dbafd43cb0a543ad70f746d157a551))
- Defer guidance intent and coalesce session statistics reads ([87b8b8a](https://github.com/sailry/sailry-harness/commit/87b8b8a5158d174cdea4ae855196416ef9fd7adf))
- Adopt upstream Responses reasoning with provider-scoped replay ([42a8360](https://github.com/sailry/sailry-harness/commit/42a836077bb91443b0f52bf0bb19c8c80edeef8c))
- Add initial plugin tools and the turn message read action ([5a9f28e](https://github.com/sailry/sailry-harness/commit/5a9f28e7239aaf187efa49c0135db62767844ead))
- Update plugins for memory 0.1.15 ([2aa88b4](https://github.com/sailry/sailry-harness/commit/2aa88b4828a8242a99abcb1a99a68223517a1e1e))
- Render background results in the shared user bubble ([349408e](https://github.com/sailry/sailry-harness/commit/349408e314a62a3615de214418c87a4fae08cc19))
- Record review and implementation agent model preferences ([004f9ea](https://github.com/sailry/sailry-harness/commit/004f9eaf20b38cd89dbbcd00506e10f02e73f7cd))
- Keep tool details by expansion state instead of viewport position ([0e4f7bf](https://github.com/sailry/sailry-harness/commit/0e4f7bf39e60de033b8304c47802b6afd7624721))
- Project the saved snapshot diff for every file mutation ([9c40d79](https://github.com/sailry/sailry-harness/commit/9c40d791a468b5f5bae874fab33e3211170d059c))
- Keep the declared preview for notebook edits ([fe07534](https://github.com/sailry/sailry-harness/commit/fe075343026c006bd4b86fb56023bca35ac26cc0))
- Persist plugin turn parts once per conversation ([26e99d5](https://github.com/sailry/sailry-harness/commit/26e99d5e02806a5eddade9804f67282b8f1ebceb))
- Update plugins for memory 0.1.16 ([5e20cdd](https://github.com/sailry/sailry-harness/commit/5e20cddcb51af99ddcb354cdc636b24ea44e0b66))
- Fix memory writes, repository reconnects, and context deduplication ([0a4eee7](https://github.com/sailry/sailry-harness/commit/0a4eee75ae257ce802942adfcc9086134cda762f))
- Keep visible conversation details active during terminal focus ([b6ce18b](https://github.com/sailry/sailry-harness/commit/b6ce18b522061a735711f607269166124ea0b039))
- Center the code copy button in its header ([8442603](https://github.com/sailry/sailry-harness/commit/84426036c8ed61c20e018f13a0727a102aa0555f))
- Update commands plugin to omit startup-only details ([52b5e8a](https://github.com/sailry/sailry-harness/commit/52b5e8aae48f3066739d7a87a43455e3a17ed8f2))
- Serve the full Kit icon catalog for plugin icons ([a3d28d1](https://github.com/sailry/sailry-harness/commit/a3d28d171db0b22cfa1d1fefb1a56df767d8594a))
- Show connector account loading and retry states ([e88752c](https://github.com/sailry/sailry-harness/commit/e88752c8895d2c69d9487b22e460a2c47aa93e5f))
- Show focus rings only for keyboard focus ([65d52f4](https://github.com/sailry/sailry-harness/commit/65d52f45d80389e6c9e7076b0784d9576b560e36))
- Fix empty image placeholder, image clicks and math-paragraph inline code ([bc2c28a](https://github.com/sailry/sailry-harness/commit/bc2c28aa7cbe8ce1cf18e17b1d52a673e839063f))
- Preview document images on click only when read-only ([7f6eb75](https://github.com/sailry/sailry-harness/commit/7f6eb754d966226c186e242898637590dfac0cfb))
- Hide agent-only parts from conversation search and clean result snippets ([9e7394b](https://github.com/sailry/sailry-harness/commit/9e7394b2364261b066b94f424b198daa583a24ee))
- Polish file change review, search rows and activity spacing ([61237c0](https://github.com/sailry/sailry-harness/commit/61237c00b89081b10dd7020f23f2b4326f4d4a74))
- Stop recording notifications for terminal exits ([b33d66c](https://github.com/sailry/sailry-harness/commit/b33d66c3f9775cea69d8007c717be7ebbbc31cb3))
- Update plugins for terminal exits, statistics tooltips and memory deletion ([00b4e1a](https://github.com/sailry/sailry-harness/commit/00b4e1aa89db4d12be1961fbe7931e0a92cf0fa7))
- Animate shell sidebar and details panes in both directions ([21a252f](https://github.com/sailry/sailry-harness/commit/21a252fff8a06e967d8b2cc546e078160971a096))
- Skip inaccessible Git subdirectories silently ([d9e1b07](https://github.com/sailry/sailry-harness/commit/d9e1b072b3b8a0ace4679562d2d7d74f77ae1fd0))
- Acknowledge guidance queued with different settings ([c67d01c](https://github.com/sailry/sailry-harness/commit/c67d01c3111f9b345d9fb79a609b431c34d2d2a5))
- Keep conversation image previews enabled ([b347385](https://github.com/sailry/sailry-harness/commit/b347385b2f906e5d690a2cc5d86179f058c497eb))
- Make background command follow-up opt in ([aa780a7](https://github.com/sailry/sailry-harness/commit/aa780a7f2726e53f02ead971c754c2b3cdf1907a))
- Update commands plugin for optional background follow-up ([2c0a341](https://github.com/sailry/sailry-harness/commit/2c0a341041867c5dbee744419b5011eaff4c113b))
- Cover terminal redraws during sidebar resizing ([b60a924](https://github.com/sailry/sailry-harness/commit/b60a924d7c7fd9be8c3c0edb2932b530120f5437))
- Skip inaccessible tracked directories in Git status ([9076e03](https://github.com/sailry/sailry-harness/commit/9076e03032ed2c9601f6596ef566c4d6ad5a0f7d))
- Repair strict lint and current history fixtures ([8b26edf](https://github.com/sailry/sailry-harness/commit/8b26edfb46127a54a1ed6848d76ce9891886392e))
- Pin restored plugin catalog lineage ([5890688](https://github.com/sailry/sailry-harness/commit/589068889bf577687e192f2b2b2374728dc74c91))
- Prepare 0.1.0-alpha.8 ([841b4f4](https://github.com/sailry/sailry-harness/commit/841b4f4c3f654cd927d66a8c90693397810c46d2))
- Match Dart paging fixture to on-demand history ([bc0dd35](https://github.com/sailry/sailry-harness/commit/bc0dd353621d95708fb7dc446e3e41944e4e730f))
- Remove timing races from restore and reminder fixtures ([a11fe9c](https://github.com/sailry/sailry-harness/commit/a11fe9ceea19be189d87711ee13723fa45d6d6cc))
- Remove the synthetic model retry prompt ([1ce14a3](https://github.com/sailry/sailry-harness/commit/1ce14a3e27080fac854e2fe18136c84315630a1d))
- Expose hook timeout test outcomes ([7ad7ec5](https://github.com/sailry/sailry-harness/commit/7ad7ec57e13d9540f029484a3b622de929b4e328))
- Render mobile transcript steps as desktop tool rows ([c7339d3](https://github.com/sailry/sailry-harness/commit/c7339d3e78d256c36f1e284963922889b717d354))
- Add the mobile composer activity strip ([4d2c4fe](https://github.com/sailry/sailry-harness/commit/4d2c4fec112fa1ed76fa013a61d97433d7906c40))
- Redesign the mobile conversation list ([24104e2](https://github.com/sailry/sailry-harness/commit/24104e29a2ad3305b17f120f2b770ef55b2f9e0c))
- Use the singular step label for one step ([718c5d7](https://github.com/sailry/sailry-harness/commit/718c5d7b8a261fa5e5cb88a336522818268ea2f2))
- Handle missing requested tool paths without masking other failures ([75bc9f5](https://github.com/sailry/sailry-harness/commit/75bc9f5b24514141ebd0c816d2212334e4ffe58d))
- Await derived usage in mobile contract fixtures ([a6ca46b](https://github.com/sailry/sailry-harness/commit/a6ca46b83ca105ada2f9b57adb9d76547fed1db4))
- Open mobile conversations without stalling the route transition ([7c3c796](https://github.com/sailry/sailry-harness/commit/7c3c796cecd918db00c73a4281069dd26c05231e))
- Align product versions with the Node status capability ([20b423a](https://github.com/sailry/sailry-harness/commit/20b423add425f44c338be0a02aef7de752c2be5c))
- Remove roles and memory settings from mobile ([3717ec9](https://github.com/sailry/sailry-harness/commit/3717ec926d7d60a0f0ba88e2ce0f01d0b7160a45))
- Add Node-owned status protocol and Client feed ([fb711a0](https://github.com/sailry/sailry-harness/commit/fb711a053e11cec422387f6500a49c6bb2086bbc))
- Compute scoped plugin status on the execution Node ([1556017](https://github.com/sailry/sailry-harness/commit/15560178ff54059dbe6b3ab9a20373179203675b))
- Pin Git and Goals packages with Node-owned status ([7bdd23f](https://github.com/sailry/sailry-harness/commit/7bdd23f6c3fbb4f8c31ba4b0961718d870f15989))
- Render Node status in Desktop contributions ([26e4371](https://github.com/sailry/sailry-harness/commit/26e4371265fa20e193c589a73286a7c2d1f6c0a7))
- Preserve status intents and uncertain action recovery ([ef6cdbb](https://github.com/sailry/sailry-harness/commit/ef6cdbbb9eb168809a83d82bbdf20486671eefb2))
- Show Node-owned status in the Mobile controller ([ea6a09c](https://github.com/sailry/sailry-harness/commit/ea6a09cccd21751b8abed451b66fcf16580a13fe))
- Retain uncertain status retries and refresh moved scopes ([76a8235](https://github.com/sailry/sailry-harness/commit/76a82356b1e57166ae3c11d9f8ef1874b011b216))
- Show an optional scope label in the mobile project bar ([8b35210](https://github.com/sailry/sailry-harness/commit/8b352105431208b99626a64bec6e34fa7e9a63e7))
- Redesign the mobile usage page around the desktop statistics data ([0f80c6f](https://github.com/sailry/sailry-harness/commit/0f80c6faaa9ad31928582d71ebcf85fd4dfc2b27))
- Drop status retries when their captured contribution disappears ([b5283bd](https://github.com/sailry/sailry-harness/commit/b5283bd02835d55947585bfc62cd0434f867a7a4))
- Brace the status availability guard ([356be2e](https://github.com/sailry/sailry-harness/commit/356be2e8d5c5cb47113cce8043f3c1b58aa4660b))
- Recover status subscriptions after watcher failure ([ea8b395](https://github.com/sailry/sailry-harness/commit/ea8b395d9b3ddb1aa96c42e69b52b82a978cdd38))
- Preserve status read cancellation through command logging ([c906023](https://github.com/sailry/sailry-harness/commit/c906023e4d5c83a21f18e026cb5b74db8eb86c19))
- Use the builtin test attribute for status helpers ([ad2037a](https://github.com/sailry/sailry-harness/commit/ad2037ab5a7f6e7ead8165aded4eb562a9f1bbb4))
- Add mobile host metric history and formatting ([9695cc9](https://github.com/sailry/sailry-harness/commit/9695cc985d9a81b389d127dd00e82207f1516652))
- Redesign the mobile hosts page around host cards and charts ([79f586f](https://github.com/sailry/sailry-harness/commit/79f586f70d162b5e8e0073e94ae9db194efed683))
- Add mobile Git and file presentation helpers ([27ee961](https://github.com/sailry/sailry-harness/commit/27ee961aeb7c2bf8546643ae2e543680e882fde6))
- Redesign the mobile resources pages ([b374df0](https://github.com/sailry/sailry-harness/commit/b374df036bbf4573bc2ed9a8020df72ce82f21d4))
- Load network presets from GitHub and probe relay latency ([c91fdb3](https://github.com/sailry/sailry-harness/commit/c91fdb3e922dc5ea69960b685405d77dd3b8bebb))
- Load pairing relay choices from the Node catalog ([aa5c4f8](https://github.com/sailry/sailry-harness/commit/aa5c4f86f7d12652b82c2d77bfc1413bda6bf178))
- Use controller identity in wrong-target discovery test ([488e330](https://github.com/sailry/sailry-harness/commit/488e33078c1d6c791a38b84c611d02518e2c3447))
- Localize network presets for the Desktop Chinese locale ([106ac49](https://github.com/sailry/sailry-harness/commit/106ac492ca6fd433ff64126287592781623b311f))
- Share the cancellation fixture client through Arc ([4687a46](https://github.com/sailry/sailry-harness/commit/4687a464d54ba0a3f13ad85b4b9b72ef2d1ffec2))
- Initialize ring in standalone network test fixtures ([561b34b](https://github.com/sailry/sailry-harness/commit/561b34b0fa935bb9ed7cfb214eea5af87207b0f6))
- Name the default relay as Public network ([255328c](https://github.com/sailry/sailry-harness/commit/255328c341bd51616e0e3c0da867fc0c3574ad82))
- Reconnect reads after paired route refreshes ([c52c5db](https://github.com/sailry/sailry-harness/commit/c52c5dbbfd21c3d3af90ff12400c657ffaf11025))
- Allow Mobile to forget saved hosts offline ([5f1b0ff](https://github.com/sailry/sailry-harness/commit/5f1b0ff2cc32a7064872b9838d302c5bfca9a084))
- Serialize Mobile pairing and host removal ([256b152](https://github.com/sailry/sailry-harness/commit/256b1525596a46d8956ba4dcc6cc6a4d27995c4b))
- Regenerate Mobile host removal bindings ([7fa14ad](https://github.com/sailry/sailry-harness/commit/7fa14ad8f7b4e6115dc75c766cb7c0aacda1e9cd))
- Fill the pinned host tab header to its declared extent ([6f4dc52](https://github.com/sailry/sailry-harness/commit/6f4dc52acc30aa8a5e0e3296bfa188bfa58e84ff))
- Restart stopped Mobile watches on host selection ([fa21d3e](https://github.com/sailry/sailry-harness/commit/fa21d3e08e6ed5e655301782f8b25c4eb25c1ed3))
- Tidy mobile tool details, Git headers, terminal and file viewer ([798a47d](https://github.com/sailry/sailry-harness/commit/798a47d1969596a3237f63e5bdf80ab339ef68bd))
- Remove unused mobile command status copy ([89e605b](https://github.com/sailry/sailry-harness/commit/89e605ba78a9ed78e360d8fd9ab760658d8b657d))
- Keep forgotten-host assertion independent of peer debugging ([1a43eab](https://github.com/sailry/sailry-harness/commit/1a43eab5cfb42392746ee16ef8dfb2f917aa43df))
- Use glass capsules for mobile conversation chips and attachments ([10df47f](https://github.com/sailry/sailry-harness/commit/10df47fc0d719eee2d953a3535ff42cc884e6ac8))
- Reduce mobile task filters to status tabs with counts ([e360a75](https://github.com/sailry/sailry-harness/commit/e360a7512d0559448c134527ea0d839f1bf1b4f2))
- Remove Mobile tool detail disclosure ([cce8f30](https://github.com/sailry/sailry-harness/commit/cce8f30b3bf00092af5a8dae04502bcd22899a95))
- Render connection routes with Kit tags ([b4d1d72](https://github.com/sailry/sailry-harness/commit/b4d1d724e6160adcf037b2c3c7779849d3a79773))
- Check Mobile tool argument content contracts ([ee6a3c3](https://github.com/sailry/sailry-harness/commit/ee6a3c3f7cbda7123e108f4a9b2e135fe6bd7a15))
- Scope mobile tool detail reads to active surfaces ([2714e0f](https://github.com/sailry/sailry-harness/commit/2714e0f173f9d2cf45428575b6240ab0d64759c5))
- Serve runtime catalogs through Sailry website ([0f11588](https://github.com/sailry/sailry-harness/commit/0f11588f2b7fefa6b3f821f262d578a09165276b))
- Deploy website after formal release catalog publication ([fc3b76b](https://github.com/sailry/sailry-harness/commit/fc3b76b11b86b649c5516a19024b5b2b22a7ef43))
- Poll mobile host metrics inside the Node rate window ([6fed736](https://github.com/sailry/sailry-harness/commit/6fed73669fc2611808d6fcd1750f20e9365adfb8))
- Show mobile turn changes only with file changes; flatten approval card ([325f37a](https://github.com/sailry/sailry-harness/commit/325f37a178250c3f9218a06e51b15741b6108cf8))
- Merge mobile stop and send into one composer action ([ffb49b5](https://github.com/sailry/sailry-harness/commit/ffb49b5bb865f389a85acda70b4e52cd37638cb8))
- Add the Desktop chats group to mobile project pickers ([b82c11b](https://github.com/sailry/sailry-harness/commit/b82c11bde832f1315d1a5c38b75f8d8d3ab6189d))
- Rebuild the mobile model picker after the Desktop layout ([d729fcc](https://github.com/sailry/sailry-harness/commit/d729fccd2082699f2c273790a40316046b16d308))
- Use rounded plain tabs for mobile task filters ([df803a8](https://github.com/sailry/sailry-harness/commit/df803a83d286dd4bf99ce2a4c3e7a461cd51c06e))
- Collapse mobile multi-file diffs into rows; swipe to close terminals ([ef59ff0](https://github.com/sailry/sailry-harness/commit/ef59ff01f5d49c59b3623bb7e5efdfc1811ccd0b))
- Drop the settings shortcut from the mobile host sheet ([58ed189](https://github.com/sailry/sailry-harness/commit/58ed1897204af15d7c96d65986ba26c23ebb34af))
- Keep mobile host charts continuous after missed reads ([cef403b](https://github.com/sailry/sailry-harness/commit/cef403b165c6dce3c53de71286afe678ad89f5a6))
- Fade the mobile page header over scrolling content ([da9c44e](https://github.com/sailry/sailry-harness/commit/da9c44e5e4c29760372f825385df8a6317adde62))
- Redesign mobile model provider settings ([19084e5](https://github.com/sailry/sailry-harness/commit/19084e5d964273ecc99156964cfbccc82f92a804))
- Fix mobile task ownership and provider edit recovery ([864d46e](https://github.com/sailry/sailry-harness/commit/864d46ecd01ed4ab0de0416a3833523f9b8db88c))
- Fix mobile and plugin contract CI regressions ([d2c534d](https://github.com/sailry/sailry-harness/commit/d2c534da35b85d728ee688cfacedbf6069674c9d))
- Configure the iOS development signing team ([aaf8fbd](https://github.com/sailry/sailry-harness/commit/aaf8fbd5ffdba2dadb2158c4fc5047048863078e))
- Automate signed Mobile packages and TestFlight uploads ([8c7a481](https://github.com/sailry/sailry-harness/commit/8c7a481d3249a593f2cbb88eb21acd7af0fce8fb))
- Allow checked source commits in Mobile packaging ([8b146a2](https://github.com/sailry/sailry-harness/commit/8b146a207a912aaaece85571f30b2fbbb20b7a3b))
- Wait for recipient completion in guidance test ([098ab77](https://github.com/sailry/sailry-harness/commit/098ab77d5d9a11714734eceda9b72f22d6421d47))
- Publish version releases before independent product packages ([8b1f165](https://github.com/sailry/sailry-harness/commit/8b1f165b4c894d4bd616cb1f2bdace5c0b374e9a))
- Gate packages on product checks and scope release tooling CI ([dbc6df9](https://github.com/sailry/sailry-harness/commit/dbc6df98a6a19d778a41d9cf2f8e3f3259fad0da))
- Recover transient model failures without replaying agent work ([b828317](https://github.com/sailry/sailry-harness/commit/b828317fa272d0bb0a93f414fd2e1072cf5945f4))
- Add team protocol and client catalog projection ([76d276f](https://github.com/sailry/sailry-harness/commit/76d276f1cd16fe028f663d9576425c2cb57eef76))
- Store teams and capture team rosters in Node sessions ([6b2e034](https://github.com/sailry/sailry-harness/commit/6b2e03467a152bb9b93af7a0c8cac9c6cd93e377))
- Name the team save command PutTeam like PutRole ([1e9fdea](https://github.com/sailry/sailry-harness/commit/1e9fdeae3ba610a02c610376e35fc50be00569d7))
- Tie member model lookup to the snapshot lifetime ([7cda07c](https://github.com/sailry/sailry-harness/commit/7cda07c7f0f13f46b4682e2329ed16a4caff3742))
- Select teams and member models in the mobile model sheet ([3dfe76b](https://github.com/sailry/sailry-harness/commit/3dfe76b2ceb3a217098978aaab6121908664bf10))
- Reuse shared test builds and scope integration-only CI ([ed69203](https://github.com/sailry/sailry-harness/commit/ed692038e79560cff61af3b3ecced4acaccc5397))
- Add the core Teams settings page with team and role editors ([37011d7](https://github.com/sailry/sailry-harness/commit/37011d70761472c2464ef6f3f48d9579ba29b348))
- Select teams and member models in the composer model popover ([dbf5dd8](https://github.com/sailry/sailry-harness/commit/dbf5dd80730f4fbfe900204d9da89934aa04717a))
- Return the role presets menu as an element ([cdb7391](https://github.com/sailry/sailry-harness/commit/cdb73917188fda15cc6af01260b5f9a347b5a96e))
- Move delegation and Flagship into the Node core ([4f0df05](https://github.com/sailry/sailry-harness/commit/4f0df05754704579d18c86dfc20d8461fe71a88e))
- Point plugins at the catalog without delegation ([72aae91](https://github.com/sailry/sailry-harness/commit/72aae917fd85ae4c8e829e679dc9d7fea7007ed8))
- Offer Flagship in the mobile mode selector ([8efba6b](https://github.com/sailry/sailry-harness/commit/8efba6b0a138349d92b5c17b8176fc9d1268f3ad))
- List mobile work modes in a sheet so plugin modes fit ([2dd30b6](https://github.com/sailry/sailry-harness/commit/2dd30b6f22e291522afdeed9a43825cab527bb6f))
- Update code-review for Flagship team delegation ([8761c52](https://github.com/sailry/sailry-harness/commit/8761c52eaba91ab419f9e20b9efdb5d1f0c5beb3))
- Fix team concurrency controls and configuration recovery ([946088a](https://github.com/sailry/sailry-harness/commit/946088a6a9b8d097e708101b15c479d5f5ae0e92))
- Put the team list back button in a header ([b4d3298](https://github.com/sailry/sailry-harness/commit/b4d32987294fcec96ad9bc21663b619c2d80267a))
- Auto-save team concurrency with a slider above the tabs ([8f0ed61](https://github.com/sailry/sailry-harness/commit/8f0ed6104ca7570b84d088e411f226869cd3ad3a))
- Give every role and team a visible appearance ([10ede2e](https://github.com/sailry/sailry-harness/commit/10ede2ed2be5e8d7c9a041ca391426bbbd1ede6f))
- Show localized built-in role names in the model popover ([e398e55](https://github.com/sailry/sailry-harness/commit/e398e55f3cdf7e4df67b51d343707ff08eb108eb))
- Show mobile context compaction as a plain marker ([bfa93a6](https://github.com/sailry/sailry-harness/commit/bfa93a67bc344f5ed590b4672f18826fc417d045))
- Hide subagent sessions from the mobile list and drop their footer label ([692f799](https://github.com/sailry/sailry-harness/commit/692f799a41ea1d43b4687b7f8fda0c7ea73ebd1c))
- Use the regular size for the team and role create buttons ([78b675d](https://github.com/sailry/sailry-harness/commit/78b675d8b58a3d4e3a7d793fcbaf82fb5f7b6f0f))
- Put team selection in the mobile model sheet ([21580d1](https://github.com/sailry/sailry-harness/commit/21580d13c7abc1fcd3d59c5a5e68f2208ff099e8))
- Remove the unused mobile subagent footer label ([fd9380a](https://github.com/sailry/sailry-harness/commit/fd9380aa00430a9247343cbfc81c7b8de529622c))
- Fill identity avatars with their color and make the team icon a clear picker ([184528a](https://github.com/sailry/sailry-harness/commit/184528a0fbc8ee0b4ed2293bd8ebbd2ebceac7a5))
- Left-align the team list back button and line up its rows ([9ee3722](https://github.com/sailry/sailry-harness/commit/9ee37220147a04c494b90518e88f7f69d31df2c4))
- Enlarge team member avatars and align the team dialog close button ([d031156](https://github.com/sailry/sailry-harness/commit/d03115639c10de76230616f81e6264baed28f3a2))
- Show raw reasoning efforts on mobile and let models add or remove them ([494f3e7](https://github.com/sailry/sailry-harness/commit/494f3e7440532e1bbcd64006580e95abc6b4cfb5))
- Keep the lead's model features for members whose model supports them ([ddd9d44](https://github.com/sailry/sailry-harness/commit/ddd9d4485ea6798ee6a71c60098d60fcce7d922d))
- Use a thin ring for the selected member avatar ([717eccd](https://github.com/sailry/sailry-harness/commit/717eccddff8263b91bc5c60a2f775f9b9f23a8e8))
- Keep the team icon color on hover, outline Add role and drop its search ([91b14c3](https://github.com/sailry/sailry-harness/commit/91b14c33988253fcc3d31594f39dbc1ca4125e52))
- Show the connection route as plain text ([0a57a77](https://github.com/sailry/sailry-harness/commit/0a57a779d8650c5a0c17aa0b02ca1bc8a8b1fa36))
- Size team row avatars like role rows ([badf532](https://github.com/sailry/sailry-harness/commit/badf53220bea6386493d9a60820ef79d5be580e2))
- Fade unselected member avatars instead of ringing the selected one ([28707f7](https://github.com/sailry/sailry-harness/commit/28707f7cda8588be7f1be2e93fcb99a9175809c4))
- Drop the back header from the model popover team list ([335d022](https://github.com/sailry/sailry-harness/commit/335d022b5bb341c8916093793c63a5e243979e8b))
- Show only the team icon in the model popover team chooser ([9dc8937](https://github.com/sailry/sailry-harness/commit/9dc8937ee86a6eaac4ed1d57cb5e328923e50fbf))
- Update plugins for web-search 0.1.13 ([cab9e9b](https://github.com/sailry/sailry-harness/commit/cab9e9b0f17c260c9ddce49910a5ba1c574a68e4))
- Update plugins for web-search 0.1.14 ([1ed3554](https://github.com/sailry/sailry-harness/commit/1ed355465d9dcc43ecbbe4223f4c385a6aac06c3))
- Put the team chooser at the start and members at the end of the popover row ([55b6290](https://github.com/sailry/sailry-harness/commit/55b6290a02a731a95a911a9b30348e8562915424))
- Draw team and role identities as provider-sized tiles ([4d6609b](https://github.com/sailry/sailry-harness/commit/4d6609bd7329cb1e356a4ff45d0fc884890de7bc))
- Tint team and role identities with a translucent fill ([0c6d554](https://github.com/sailry/sailry-harness/commit/0c6d554506c423b12a5ce7c43fac7a3257686a50))
- Right-align popover members inside their scroll area ([082ba02](https://github.com/sailry/sailry-harness/commit/082ba0262f78227604a6db9ce68367103b8d1404))
- Show team member avatars side by side instead of stacked ([cda3ce9](https://github.com/sailry/sailry-harness/commit/cda3ce9bdd6dd0d7a27e2dfa129475345cb74145))
- Give the team lead a member-style header ([6ca2e55](https://github.com/sailry/sailry-harness/commit/6ca2e55cb494c524d76b7b1273c26e6a47232d37))
- Fix terminal environment, ownership recovery and direct closure ([1840d4a](https://github.com/sailry/sailry-harness/commit/1840d4a0a49dc72763d655eaf78eac0834f16f54))
- Add a shared modal list and use it for connection methods ([d01ab13](https://github.com/sailry/sailry-harness/commit/d01ab13c857e569d65d3cfe399d61423170673c6))
- Rewrite the team dialog as a single form with the shared list ([bfd416f](https://github.com/sailry/sailry-harness/commit/bfd416f250afc9f31285dfe201b76488320f04e5))
- Show provider models in the shared modal list ([2663006](https://github.com/sailry/sailry-harness/commit/2663006accb0d6a5494c2cb1ca1df685c1078727))
- Use the shared modal list for connector accounts and types ([ae0031c](https://github.com/sailry/sailry-harness/commit/ae0031c9a78405642c38429b83aed61088aaad8a))
- Use the shared modal list for backup records and archives ([ce9f18a](https://github.com/sailry/sailry-harness/commit/ce9f18a4f274417ad342e041d1730624e5630920))
- Use the shared modal list for skill install candidates ([8c5082d](https://github.com/sailry/sailry-harness/commit/8c5082d664152029de3b678f9265b68ae8c0be28))
- Use the shared modal list for project session records ([64495d0](https://github.com/sailry/sailry-harness/commit/64495d0db71e42109f3cf639baf32a93cb33aa39))
- Use the shared modal list for CLI launch profiles ([72ae5ac](https://github.com/sailry/sailry-harness/commit/72ae5ace2eabd29011ef11e383192d742d8ecdd8))
- Use the shared modal list for the worktree manager ([d5d6909](https://github.com/sailry/sailry-harness/commit/d5d6909bdbfc9b90061d2c58c714b3bae042091c))
- Use the shared modal list for host project sharing ([3fd8ef3](https://github.com/sailry/sailry-harness/commit/3fd8ef34d2faab181711da15febe2a232f97dd4d))
- Let modal list rows show details without a disclosure ([5c94c6c](https://github.com/sailry/sailry-harness/commit/5c94c6c4992944e0a1e56026cb9713314ecbbcce))
- Use the shared modal list for plugin updates ([d61cafd](https://github.com/sailry/sailry-harness/commit/d61cafd1963794ccae3f97a10fd6aca182dae983))
- Fix plugin form modal height and scrolling ([657ae27](https://github.com/sailry/sailry-harness/commit/657ae27989f2954d220a0ae8d79846c063a1a2fc))
- Unify model and team sessions for controllers and plugins ([8b13e13](https://github.com/sailry/sailry-harness/commit/8b13e1392b0f1a3dcd2aa8dba44fcf8a0eb46618))
- Update scheduled tasks to shared session execution ([8f87467](https://github.com/sailry/sailry-harness/commit/8f87467f6809b1dda370f43f8bf1623a025a2263))
- Fix team selectors and mobile mode controls ([f4ac27a](https://github.com/sailry/sailry-harness/commit/f4ac27a5d95d573729be67695bef4aca9330954b))
- Complete mobile team and conversation location controls ([df9f1ef](https://github.com/sailry/sailry-harness/commit/df9f1ef6c12db78b1871cb3fc6fb797f01f27de1))
- Soften default dark overlay borders ([1f19e9c](https://github.com/sailry/sailry-harness/commit/1f19e9cc9e401979d937722ae91071ad8b2049ba))
- Fix mobile subtask controls and row presentation ([4c41146](https://github.com/sailry/sailry-harness/commit/4c4114611f025fa46a41b31042a8863ff973170d))
- Fix session role captures and controller review findings ([ef61b57](https://github.com/sailry/sailry-harness/commit/ef61b57d88c8e2486959c8dfd7375b0a53918195))
- Update validated plugin packages for stable release ([f914f56](https://github.com/sailry/sailry-harness/commit/f914f569471eef1d16f3488022d2b83e5cf92140))
- Release Sailry 0.1.0 ([17c73d1](https://github.com/sailry/sailry-harness/commit/17c73d1f4b4036b0b9e5f426773aa75ae96bf334))
- Wait for completion notices in Mobile activity contract ([715bb41](https://github.com/sailry/sailry-harness/commit/715bb41a02ce324aff5934f6a842f5dc80beaf70))
- Refresh release actions and explicitly install pinned Rust ([307c7cc](https://github.com/sailry/sailry-harness/commit/307c7ccc6287c9015cf3787cc9fb86c87bc0af19))

[Full changelog](https://github.com/sailry/sailry-harness/compare/v0.1.0-alpha.6...v0.1.0)

## [0.1.0-alpha.3](https://github.com/sailry/sailry-harness/releases/tag/v0.1.0-alpha.3)

### Highlights

- Use native Turso storage with concurrent reads and short, conflict-aware
  transactions. Captured file-tool results can be persisted again without
  repeating the file operation.
- Refresh file views after checkpoint restores and release Node network
  resources on shutdown so the same endpoint can restart.
- Improve provider retry and WebSocket recovery, preserve uncertain outcomes,
  and retain useful provider error details.
- Keep plugin settings pages mounted, add Git initialization to the repository
  sidebar, and make delegated-role editors scroll completely.
- Clarify tool output and source-code headers, improve folding, and remove the
  extra partial-results footer from file changes.
- Keep structured Plan tools bundled and offer the Activity Board as a separate
  optional plugin.
- Show consistent session throughput and request latency in Statistics, with
  All selections and visible filter state. Statistics cover Sailry sessions.

### Downloads

- macOS Desktop: Apple silicon and Intel DMG installers and ZIP archives
- Sailry Host: macOS and Linux archives for arm64 and x86_64, with an installer
  and SHA-256 checksums

Storage uses new native Turso files. Existing SQLite databases are retained and
are not imported automatically.

This is an ordinary GitHub Release. Its alpha version remains in the experience
update channel.

## [0.1.0-alpha.1](https://github.com/sailry/sailry-harness/releases/tag/v0.1.0-alpha.1)

The first Sailry Harness alpha brings AI conversations and development tools into
a native workspace. Features and plugin APIs may change during alpha development.

### Highlights

- A macOS workspace with projects, conversations, files, diffs and terminals
- Local execution inside Desktop and persistent tasks on your own Sailry Host
- Plan review, guidance during execution, and delegated agents with configurable roles
- Plugins for code, research, documents, SSH and databases, with an extensible SDK
- Multiple model providers, compatible endpoints, and ChatGPT and Claude sign-in
- Document previews, conversation exports, themes, and optional speech, OCR and embedding models
- Ad-hoc macOS downloads first, followed by independent formal signing and notarization, plus Host packages for macOS and Linux

### Fixed

- Link XZ support statically so macOS packages do not depend on a developer's Homebrew libraries
- Keep background inference workers compatible with Linux systems using glibc 2.28
- Retry interrupted Apple ticket validation and recover verified successful builds during publication

### Changed

- Discover signed Desktop updates from exact-version Release catalogs and require matching Desktop and Host versions, with profile-preserving Host updates
- Reduce repeated Node and package setup in integration tests while preserving real service coverage
- Share dependency sources and compiler caches across validation and release builds within the repository cache budget

### Downloads

- Desktop: macOS 13 or later, Apple silicon and Intel
- Host: macOS and Linux, ARM64 and x64
