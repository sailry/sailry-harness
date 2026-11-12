package preview

import (
	"image"
	"image/color"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
)

// The demo's right panel: the project's files, its Git changes, branches and
// history, the session's checkpoints and documents, and a knowledge source.
// The hosts are the window's own, which the shell gives it.

const reserveGo = "package booking\n\n// Reserve holds a seat for a booking. The check and the write share one\n// serializable transaction, so two requests cannot take the same seat.\nfunc (s *Store) Reserve(ctx context.Context, seat string) error {\n\ttx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelSerializable})\n\tif err != nil {\n\t\treturn err\n\t}\n\tdefer tx.Rollback()\n\tif taken, err := s.takenTx(ctx, tx, seat); err != nil || taken {\n\t\treturn ErrSeatTaken\n\t}\n\treturn s.insertTx(ctx, tx, seat)\n}"

const rateLimitGo = "package middleware\n\n// RateLimit allows each client its plan's requests a minute.\nfunc RateLimit(limits config.Limits, store Counter) func(http.Handler) http.Handler {\n\treturn func(next http.Handler) http.Handler {\n\t\treturn http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {\n\t\t\tclient := clientID(r)\n\t\t\tif !store.Allow(client, limits.For(client)) {\n\t\t\t\tw.Header().Set(\"Retry-After\", \"60\")\n\t\t\t\thttp.Error(w, \"rate limit exceeded\", http.StatusTooManyRequests)\n\t\t\t\treturn\n\t\t\t}\n\t\t\tnext.ServeHTTP(w, r)\n\t\t})\n\t}\n}"

func (b *demo) workspace() *Workspace {
	f := func(path, text string) *FileNode {
		return &FileNode{Name: path[lastSlash(path)+1:], Path: path, Text: text}
	}
	dir := func(path string, kids ...*FileNode) *FileNode {
		return &FileNode{Name: path[lastSlash(path)+1:], Path: path, Children: kids}
	}
	w := &Workspace{}
	w.Files = dir("harbor-api",
		dir("harbor-api/cmd", dir("harbor-api/cmd/harbor",
			f("harbor-api/cmd/harbor/main.go", "package main\n\nfunc main() {\n\tcfg := config.MustLoad()\n\tlog.Fatal(server.New(cfg).ListenAndServe())\n}"))),
		dir("harbor-api/docs", f("harbor-api/docs/booking.md", b.t("panel.file.booking"))),
		dir("harbor-api/internal",
			dir("harbor-api/internal/booking",
				f("harbor-api/internal/booking/reserve.go", reserveGo),
				f("harbor-api/internal/booking/reserve_test.go", "package booking\n\nfunc TestReserveSeat(t *testing.T) {\n\tstore := newStore(t)\n\tvar wg sync.WaitGroup\n\tfor range 8 {\n\t\twg.Go(func() { _ = store.Reserve(ctx, \"14C\") })\n\t}\n\twg.Wait()\n}")),
			dir("harbor-api/internal/http", f("harbor-api/internal/http/router.go", "package http\n\n"+routerGo)),
			dir("harbor-api/internal/middleware", f("harbor-api/internal/middleware/ratelimit.go", rateLimitGo))),
		f("harbor-api/README.md", b.t("panel.file.readme")),
		f("harbor-api/go.mod", "module github.com/harbor/harbor-api\n\ngo 1.27.3\n\nrequire github.com/go-chi/chi/v5 v5.2.3"),
	)
	w.Changes = []FileChange{
		change("internal/booking/reserve.go", "M", hunk(5, 5,
			" func (s *Store) Reserve(ctx context.Context, seat string) error {",
			"-\ttaken, err := s.taken(ctx, seat)", "-\tif err != nil || taken {", "-\t\treturn ErrSeatTaken",
			"+\ttx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelSerializable})", "+\tif err != nil {", "+\t\treturn err", "+\t}",
			"+\tdefer tx.Rollback()", "+\tif taken, err := s.takenTx(ctx, tx, seat); err != nil || taken {", "+\t\treturn ErrSeatTaken", " \t}")),
		change("internal/http/router.go", "M", hunk(4, 4,
			" \tr.Route(\"/bookings\", func(r chi.Router) {", "-\t\tr.Post(\"/\", s.createBooking)",
			"+\t\tr.With(middleware.RateLimit(s.cfg.Limits, s.counters)).Post(\"/\", s.createBooking)", " \t\tr.Get(\"/{id}\", s.getBooking)")),
		change("internal/middleware/ratelimit.go", "A", hunk(0, 1,
			"+package middleware", "+", "+// RateLimit allows each client its plan's requests a minute.",
			"+func RateLimit(limits config.Limits, store Counter) func(http.Handler) http.Handler {", "+\treturn limiter{limits, store}.wrap", "+}")),
		change("internal/middleware/burst.go", "D", hunk(1, 0, "-package middleware", "-", "-// burst is replaced by RateLimit.", "-var burst = 20")),
	}
	w.Changes[0].Staged = "M"
	w.Changes[1].Staged, w.Changes[1].Unstaged = "M", "M"
	w.Changes[2].Untracked = true
	w.Changes[3].Unstaged = "D"
	w.Repo = Repository{
		Branch: "feature/rate-limit", Upstream: "origin/feature/rate-limit",
		Branches: []Branch{
			{Name: "feature/rate-limit", Current: true, Commit: "5c1e9a40b7d2", Tracking: "↑2"},
			{Name: "main", Commit: "a3f07c18e964"},
			{Name: "fix/reserve-race", Commit: "e81b2d6f03c5", Tracking: "↓1"},
			{Name: "origin/main", Remote: true, Commit: "a3f07c18e964"},
			{Name: "origin/feature/rate-limit", Remote: true, Commit: "9d42f6b1a0e7"},
		},
		Remotes: []Remote{{Name: "origin", URL: "https://git.harbor.example/harbor/harbor-api.git"}},
		Stashes: []string{"WIP on main: a3f07c1 Tune the booking indexes"},
		Tags:    []string{"v2.3.1", "v2.4.0-rc.2"},
		// Newest first, every child before its parents: the branch took main
		// in, main took the reservation fix in, and both left the release
		// candidate. The history goes on past the last commit listed.
		Commits: []Commit{
			{ID: "5c1e9a40b7d2f381", Message: "Add a token bucket for booking requests", Author: "Maya Lindqvist", Refs: []string{"HEAD", "heads/feature/rate-limit"}, Parents: []string{"c4d81f0a7e3b5962"}},
			{ID: "c4d81f0a7e3b5962", Message: "Merge branch 'main' into feature/rate-limit", Author: "Maya Lindqvist", Refs: []string{}, Parents: []string{"9d42f6b1a0e7c254", "a3f07c18e964b0d5"}},
			{ID: "9d42f6b1a0e7c254", Message: "Read plan limits from the configuration\n\nAnonymous, free and pro clients each get their own budget.", Author: "Maya Lindqvist", Refs: []string{"remotes/origin/feature/rate-limit"}, Parents: []string{"2e6b0d94c1f7a835"}},
			{ID: "2e6b0d94c1f7a835", Message: "Sketch the rate limit middleware", Author: "Maya Lindqvist", Refs: []string{}, Parents: []string{"71c5d09e2ab84f63"}},
			{ID: "a3f07c18e964b0d5", Message: "Tune the booking indexes", Author: "Jonas Okafor", Refs: []string{"heads/main", "remotes/origin/main"}, Parents: []string{"f19c3a5d8b60e742"}},
			{ID: "f19c3a5d8b60e742", Message: "Merge pull request #212 from fix/reserve-race", Author: "Jonas Okafor", Refs: []string{}, Parents: []string{"71c5d09e2ab84f63", "e81b2d6f03c5a917"}},
			{ID: "e81b2d6f03c5a917", Message: "Reserve seats in one serializable transaction", Author: "Jonas Okafor", Refs: []string{"heads/fix/reserve-race"}, Parents: []string{"b60a4e17d92c3f58"}},
			{ID: "b60a4e17d92c3f58", Message: "Reproduce the double booking in a test", Author: "Jonas Okafor", Refs: []string{}, Parents: []string{"71c5d09e2ab84f63"}},
			{ID: "71c5d09e2ab84f63", Message: "Release 2.4.0-rc.2", Author: "Maya Lindqvist", Refs: []string{"tags/v2.4.0-rc.2"}, Parents: []string{"08d3b7f5a6c41e29"}},
			{ID: "08d3b7f5a6c41e29", Message: "Paginate the invoice list", Author: "Jonas Okafor", Refs: []string{}, Parents: []string{"4a9e6c2d17b3f085"}},
			{ID: "4a9e6c2d17b3f085", Message: "Release 2.3.1", Author: "Maya Lindqvist", Refs: []string{"tags/v2.3.1"}, Parents: []string{"d5f20b8e3a7c1946"}},
		},
	}
	w.Checkpoints = []Checkpoint{
		{ID: "cp-3", Turn: b.t("limit.1.user"), Time: Now.Add(-3 * time.Minute), Files: []string{"internal/middleware/ratelimit.go", "internal/http/router.go"}, Added: 127, Removed: 2},
		{ID: "cp-2", Turn: b.t("flaky.1.user"), Time: Now.Add(-36 * time.Minute), Files: []string{"internal/booking/reserve.go", "internal/booking/reserve_test.go", "docs/booking.md"}, Added: 34, Removed: 6},
		{ID: "cp-1", Turn: b.t("invoice.0.user"), Time: Now.Add(-4 * time.Hour), Files: []string{"migrations/0041_invoices_v2.sql"}, Added: 86},
	}
	w.Documents = []Document{
		{ID: "plan", Title: b.t("panel.plan.title"), Path: "plans/rate-limiting.md", Text: b.t("panel.plan.text")},
		{ID: "report", Title: b.t("panel.report.title"), Path: "reports/load-test.md", Text: b.t("panel.report.text")},
	}
	w.Citations = []Citation{{
		Link: "sailry-knowledge://handbook/rate-limits?revision=4", Title: b.t("panel.source.title"), Host: "local_host_name",
		Path: "handbook/platform/rate-limits.md", Start: 8, End: 24,
		Pages: []string{b.t("panel.source.page1"), b.t("panel.source.page2")},
	}}
	return w
}

func lastSlash(path string) int {
	for i := len(path) - 1; i >= 0; i-- {
		if path[i] == '/' {
			return i
		}
	}
	return -1
}

// demoChart is the picture a demo message attaches and a report exports: a
// bar chart drawn here, as nothing is read from disk.
var demoChart = sync.OnceValue(func() cv.Image {
	const w, h = 560, 320
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	fill := func(x0, y0, x1, y1 int, c color.RGBA) {
		for y := max(y0, 0); y < min(y1, h); y++ {
			for x := max(x0, 0); x < min(x1, w); x++ {
				img.SetRGBA(x, y, c)
			}
		}
	}
	fill(0, 0, w, h, color.RGBA{250, 250, 252, 255})
	for i := range 5 {
		y := 40 + i*56
		fill(40, y, w-24, y+1, color.RGBA{226, 228, 235, 255})
	}
	bars := []int{38, 52, 47, 61, 190, 58, 44, 49, 63, 172, 55, 41}
	for i, v := range bars {
		c := color.RGBA{96, 132, 240, 255}
		if v > 120 {
			c = color.RGBA{232, 96, 88, 255}
		}
		x := 56 + i*40
		fill(x, 264-v, x+24, 264, c)
	}
	fill(40, 264, w-24, 266, color.RGBA{150, 154, 166, 255})
	return cv.Image{ID: "demo-ci-failures", Name: "ci-failures.png", Width: w, Height: h, Source: ui.NewBitmap(img)}
})
