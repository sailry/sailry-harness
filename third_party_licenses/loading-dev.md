# loading-dev indicators

Desktop's loading marks (`kit.Loader`, `kit.Busy`, `kit.Indicator` in
`internal/desktop/kit`) are the indicators of the `plugins/loading` package
of the pinned MyGo version, linked unmodified as part of the
`github.com/egoist/mygo` module: no source of it is copied into this
repository.

That package follows the indicators of loading-dev
(https://github.com/jakubkrehel/loading, https://loading.dev): their shapes,
keyframes and timings (`plugins/loading/NOTICE.md` in the MyGo module).
loading-dev is licensed under MIT, Copyright (c) 2026 Jakub Krehel; the
notice is retained in [loading-dev-MIT.txt](loading-dev-MIT.txt).

Two of loading-dev's indicators that the pinned package does not have, Loop
and Chevrons, are drawn by `internal/desktop/kit/marks.go` from loading-dev's
own path data, keyframes and timings (`src/loop.tsx`, `src/chevrons.tsx`),
under the same notice.

Update this file and the retained notice with the MyGo pin when the package's
notice changes.
