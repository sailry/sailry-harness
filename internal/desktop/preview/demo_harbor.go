package preview

import (
	"fmt"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// The demo's harbor-api project, a booking service in Go: a run in progress
// that waits on approvals and a question, a fixed flaky test with
// attachments, tool results of every part and a changes card, and a
// migration that was interrupted, retried and failed. Its release, which
// waits on approvals in every state, and its plan awaiting review are in
// demo_waiting.go.

const routerGo = `func (s *Server) routes() http.Handler {
	r := chi.NewRouter()
	r.Use(middleware.RequestID, middleware.Recover)
	r.Route("/bookings", func(r chi.Router) {
		r.Post("/", s.createBooking)
		r.Get("/{id}", s.getBooking)
	})
	return r
}`

// rateLimit is the conversation the demo opens on: a running reply with
// subagents, a task in progress, calls awaiting approval, a question, a
// queue, background commands and a plugin's badge.
func (b *demo) rateLimit() *Conversation {
	const id = DemoStart
	first := b.user(id+"/0", "limit.0.user")
	read := b.call(first.ID, "read", "book-open", "internal/http/router.go")
	read.Output = output(routerGo)
	limiter := b.call(first.ID, "search", "search", `"RateLimit" in internal/`)
	limiter.Output = paths("internal/middleware/throttle.go", "internal/middleware/burst.go")
	client := b.call(first.ID, "search", "search", `"X-Client-ID" in internal/`)
	client.Output = paths("internal/http/auth.go", "internal/http/router.go", "internal/booking/handler.go")
	first.Reply = done(cv.Completed, 46)
	first.Reply.Blocks = []cv.Block{reasoning(b.t("limit.0.think")), tools(read, limiter, client), text(b.t("limit.0.answer"))}

	run := b.user(id+"/1", "limit.1.user")
	config := b.call(run.ID, "read", "book-open", "internal/config/config.go")
	config.Output = output("type Limits struct {\n\tAnonymous int `yaml:\"anonymous\"`\n\tFree      int `yaml:\"free\"`\n\tPro       int `yaml:\"pro\"`\n}")
	test := b.call(run.ID, "command", "command", "go test ./internal/middleware/...")
	test.Running = true
	write := b.call(run.ID, "write", "writing", "internal/middleware/ratelimit.go")
	write.Pending, write.State = true, cv.Pending
	migrate := b.call(run.ID, "command", "command", "make migrate")
	migrate.Pending, migrate.State = true, cv.Pending
	load := b.t("limit.child.load")
	run.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(72), Phase: cv.Generating}
	// A message whose reply still runs is not edited, forked or rewound.
	run.Controls = cv.Controls{NoEdit: true, NoFork: true, NoRewind: true}
	run.Reply.Blocks = []cv.Block{
		reasoning(b.t("limit.1.think")),
		tools(config, test),
		{Kind: cv.ChildrenBlock, Children: []cv.Child{
			{ID: run.ID + "/load", Title: load, Model: demoFast, Effort: "low", Status: cv.ChildRunning},
			{ID: run.ID + "/audit", Title: b.t("limit.child.audit"), Model: demoFast, Effort: "low", Status: cv.ChildCompleted},
			{ID: run.ID + "/docs", Title: b.t("limit.child.docs"), Model: DemoModel, Effort: "medium", Status: cv.ChildQueued},
		}},
		{Kind: cv.ProgressBlock, Steps: []cv.Step{
			{Text: b.t("limit.step.bucket"), State: cv.StepCompleted},
			{Text: b.t("limit.step.router"), State: cv.StepInProgress},
			{Text: b.t("limit.step.config"), State: cv.StepPending},
			{Text: b.t("limit.step.tests"), State: cv.StepPending},
		}},
		tools(write, migrate),
		question(run.ID+"/ask", cv.QuestionPending, b.t("limit.ask.prompt"), nil),
	}

	c := b.conv(id, "limit.title", Running, 2*time.Minute, cv.Usage{Tokens: 48200, Speed: 71.4, Cost: 0.0312, Cached: 36400, Context: 0.24}, first, run)
	c.Worktree, c.WorktreePath = "feature/rate-limit", "~/Web/harbor-api-rate-limit"
	c.Waiting = composer.Interactions{
		Approvals: []composer.Approval{
			{ID: write.ID, Title: b.t("tool.write"), Context: "~/Web/harbor-api-rate-limit", Remember: true, Changes: []composer.Change{
				{Path: "internal/middleware/ratelimit.go", Kind: composer.Added, Added: 118},
				{Path: "internal/http/router.go", Kind: composer.Modified, Added: 9, Removed: 2},
				{Path: "internal/middleware/throttle.go", Destination: "internal/middleware/legacy_throttle.go", Kind: composer.Renamed},
				{Path: "internal/middleware/burst.go", Kind: composer.Deleted, Removed: 64},
				{Path: "internal/config/limits.yaml", Kind: composer.Added, Added: 420, Truncated: true},
			}},
			{ID: migrate.ID, Title: b.t("tool.command"), Context: "~/Web/harbor-api-rate-limit", Description: "make migrate", Code: true,
				Remember: true, Prefix: "make", Notices: []string{b.t("limit.approval.notice")}},
			{ID: run.ID + "/health", Title: b.t("tool.command"), Context: "~/Web/harbor-api-rate-limit",
				Description: "curl -s https://staging.harbor.example/healthz", Code: true, Unrestricted: true},
			{ID: run.ID + "/k6", Child: load, Title: b.t("tool.command"), Context: "~/Web/harbor-api-rate-limit",
				Description: "k6 run load/bookings.js --vus 200", Code: true, Remember: true, Prefix: "k6 run"},
		},
		Question: &composer.Ask{ID: run.ID + "/ask", Prompt: b.t("limit.ask.prompt"),
			Options: []string{b.t("limit.ask.ten"), b.t("limit.ask.thirty"), b.t("limit.ask.block")}, Other: true},
	}
	c.Queue = []composer.Queued{
		{ID: id + "/queued-header", Text: b.t("limit.queue.header")},
		{ID: id + "/queued-redis", Text: b.t("limit.queue.redis"), Guided: true, Joining: true},
		{ID: id + "/queued-sheet", Text: b.t("limit.queue.sheet"), Attachments: []*composer.Attachment{
			{ID: "queued-limits", Name: "plan-limits.csv", Mime: "text/csv", Size: 18 << 10, Path: "~/Downloads/plan-limits.csv", Status: composer.Ready, Progress: 1}}},
		{ID: id + "/queued-compact", Compaction: true},
		{ID: id + "/queued-notice", Text: "go vet ./...", Notice: true},
	}
	start := time.Now()
	c.Commands = []composer.Command{
		{ID: "cmd-harbor", Command: "go run ./cmd/harbor --port 8080", Started: start.Add(-14 * time.Minute), Ports: []int{8080},
			Output: "harbor listening on :8080\nPOST /bookings 201 12ms\nPOST /bookings 429 0ms"},
		{ID: "cmd-k6", Command: "k6 run load/bookings.js --vus 50", Started: start.Add(-80 * time.Second), Status: composer.CommandStopping,
			Output: "running (1m20.0s), 50/50 VUs, 18240 complete iterations"},
	}
	c.Badges = []composer.StatusBadge{b.goalBadge("goal.running", kit.Info, composer.Text{Label: "8m"},
		"goal.text", "0 / 25", "8m", "48.2k", "pause", "edit", "clear")}
	return c
}

// flakyTest is a message with a picture and a file, then tool results of
// every part: text, paths, a failure, a tool's own failure, a diff, a long
// command output and a table cut short; and the changes card.
func (b *demo) flakyTest() *Conversation {
	const id = "harbor-api/flaky-test"
	first := b.user(id+"/0", "flaky.0.user")
	first.Images = []cv.Image{demoChart()}
	first.Files = []cv.File{{ID: first.ID + "/log", Path: "logs/ci-run-4812.txt", Mime: "text/plain", Size: 9 << 10, Kind: cv.TextFile,
		Text: "=== RUN   TestReserveSeat\n    reserve_test.go:88: seat 14C reserved twice\n--- FAIL: TestReserveSeat (0.03s)\nFAIL\tgithub.com/harbor/harbor-api/internal/booking\t1.204s"}}
	read := b.call(first.ID, "read", "book-open", "internal/booking/reserve_test.go")
	read.Output = output("func TestReserveSeat(t *testing.T) {\n\tstore := newStore(t)\n\tvar wg sync.WaitGroup\n\tfor range 8 {\n\t\twg.Go(func() { _ = store.Reserve(ctx, \"14C\") })\n\t}\n\twg.Wait()\n\tif n := store.Count(ctx, \"14C\"); n != 1 {\n\t\tt.Fatalf(\"seat 14C reserved %d times\", n)\n\t}\n}")
	search := b.call(first.ID, "search", "search", `"func (s *Store) Reserve" in internal/`)
	search.Output = paths("internal/booking/reserve.go")
	fail := b.call(first.ID, "command", "command", "go test ./internal/booking -run TestReserveSeat -count=50")
	fail.Failed, fail.State, fail.Error = true, cv.StateFailed, "exit status 1"
	fail.Output = &cv.Output{Failure: "--- FAIL: TestReserveSeat (0.03s)\n    reserve_test.go:88: seat 14C reserved 2 times\nFAIL\tgithub.com/harbor/harbor-api/internal/booking\t1.188s"}
	fetch := b.call(first.ID, "fetch", "globe", "https://pkg.go.dev/database/sql#TxOptions")
	fetch.Warning, fetch.Error = true, b.t("flaky.fetch.error")
	fetch.Output = &cv.Output{Failure: b.t("flaky.fetch.failure"), ToolFailure: true}
	first.Reply = done(cv.Completed, 52)
	first.Reply.Blocks = []cv.Block{reasoning(b.t("flaky.0.think")), tools(read, search), tools(fail), tools(fetch), text(b.t("flaky.0.answer"))}

	fix := b.user(id+"/1", "flaky.1.user")
	edit := b.call(fix.ID, "edit", "pencil", "internal/booking/reserve.go")
	edit.Output = &cv.Output{Diff: &cv.Diff{Path: "internal/booking/reserve.go", Added: 7, Removed: 3, Lines: []cv.DiffLine{
		{Op: ' ', Text: "func (s *Store) Reserve(ctx context.Context, seat string) error {"},
		{Op: '-', Text: "\ttaken, err := s.taken(ctx, seat)"},
		{Op: '-', Text: "\tif err != nil || taken {"},
		{Op: '-', Text: "\t\treturn ErrSeatTaken"},
		{Op: '+', Text: "\ttx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelSerializable})"},
		{Op: '+', Text: "\tif err != nil {"},
		{Op: '+', Text: "\t\treturn err"},
		{Op: '+', Text: "\t}"},
		{Op: '+', Text: "\tdefer tx.Rollback()"},
		{Op: '+', Text: "\tif taken, err := s.takenTx(ctx, tx, seat); err != nil || taken {"},
		{Op: '+', Text: "\t\treturn ErrSeatTaken"},
		{Op: ' ', Text: "\t}"},
	}}}
	var lines []string
	for _, pkg := range []string{"booking", "config", "http", "middleware", "payment", "store"} {
		for i := range 20 {
			lines = append(lines, fmt.Sprintf("=== RUN   Test%s%02d\n--- PASS: Test%s%02d (0.0%ds)", strings.ToUpper(pkg[:1])+pkg[1:], i, strings.ToUpper(pkg[:1])+pkg[1:], i, i%7))
		}
		lines = append(lines, "ok  \tgithub.com/harbor/harbor-api/internal/"+pkg+"\t0.412s")
	}
	suite := b.call(fix.ID, "command", "command", "go test ./... -race -count=20")
	suite.Output = &cv.Output{Text: strings.Join(lines, "\n"), Command: true}
	query := b.call(fix.ID, "query", "tasks", "SELECT status, count(*) FROM reservations GROUP BY status")
	query.Output = &cv.Output{Table: &cv.Table{Columns: []string{"status", "count"}, Rows: [][]string{
		{"confirmed", "18 204"}, {"held", "312"}, {"expired", "2 951"},
	}}, Partial: true, Notices: []string{b.t("flaky.table.notice")}}
	fix.Reply = done(cv.Completed, 98)
	fix.Reply.Blocks = []cv.Block{tools(edit, suite, query), text(b.t("flaky.1.answer"))}
	fix.Reply.Changes = &cv.Changes{Files: []cv.ChangedFile{
		{Path: "internal/booking/reserve.go", Added: 7, Removed: 3},
		{Path: "internal/booking/reserve_test.go", Added: 22, Removed: 1},
		{Path: "docs/booking.md", Added: 5, Removed: 2},
	}, Undone: map[string]bool{"docs/booking.md": true}}
	return b.conv(id, "flaky.title", Idle, 35*time.Minute, cv.Usage{Tokens: 31800, Speed: 64.2, Cost: 0.0188, Cached: 22100, Context: 0.16}, first, fix)
}

// invoices is a plan with a step in every state, calls that were denied,
// interrupted and not executed, background results in every outcome,
// commands sent to the background (running, ended, failed), model
// retries that gave up, and runs that were continued, cancelled and failed.
func (b *demo) invoices() *Conversation {
	const id = "harbor-api/invoices"
	plan := b.user(id+"/0", "invoice.0.user")
	denied := b.call(plan.ID, "command", "command", "psql -f migrations/0042_drop_legacy.sql")
	denied.Warning, denied.State, denied.Error = true, cv.Denied, b.t("invoice.denied")
	cut := b.call(plan.ID, "write", "writing", "migrations/0043_currency.sql")
	cut.Warning, cut.State = true, cv.StateInterrupted
	skipped := b.call(plan.ID, "write", "writing", "migrations/0044_indexes.sql")
	skipped.Warning, skipped.State = true, cv.NotExecuted
	plan.Reply = done(cv.Interrupted, 131)
	plan.Reply.Blocks = []cv.Block{
		{Kind: cv.ProgressBlock, Steps: []cv.Step{
			{Text: b.t("invoice.step.inventory"), State: cv.StepCompleted},
			{Text: b.t("invoice.step.archive"), State: cv.StepSkipped},
			{Text: b.t("invoice.step.backfill"), State: cv.StepFailed},
			{Text: b.t("invoice.step.signoff"), State: cv.StepBlocked},
			{Text: b.t("invoice.step.dual"), State: cv.StepPaused},
			{Text: b.t("invoice.step.reads"), State: cv.StepInProgress},
			{Text: b.t("invoice.step.drop"), State: cv.StepPending},
		}},
		tools(denied, cut, skipped),
		text(b.t("invoice.0.answer")),
	}

	results := cv.Turn{ID: id + "/1", Notices: []cv.Notice{
		{Title: "go test ./internal/billing/...", Outcome: cv.OutcomeCompleted},
		{Title: b.t("invoice.notice.verify"), Agent: true, Outcome: cv.OutcomeFailed},
		{Title: "pg_dump --table invoices", Outcome: cv.OutcomeCancelled},
		{Title: b.t("invoice.notice.totals"), Agent: true, Outcome: cv.OutcomeStopped},
		{Title: "make lint", Outcome: cv.OutcomeTimedOut},
		{Title: "npm run docs", Outcome: cv.OutcomeInterrupted},
		{Title: b.t("invoice.notice.rollback"), Agent: true, Outcome: cv.OutcomeUnknown},
		{Title: "watch -n 5 psql -c 'select count(*) from invoices_v2'", Outcome: cv.OutcomeStalled},
	}, Reply: done(cv.Completed, 4)}
	results.Reply.Notice = true

	why := b.user(id+"/2", "invoice.2.user")
	why.Reply = done(cv.Failed, 75)
	why.Reply.Blocks = []cv.Block{
		text(b.t("invoice.2.partial")),
		{Kind: cv.RetryBlock, Retry: &cv.Retry{Attempt: 2, Limit: 5, Reason: "chat_model_rate_limited"}},
		{Kind: cv.RetryBlock, Retry: &cv.Retry{Attempt: 3, Limit: 5, Reason: "chat_model_usage_exhausted", Stopped: true}},
		{Kind: cv.RetryBlock, Retry: &cv.Retry{Attempt: 5, Limit: 5, Reason: "chat_model_server_overloaded", Exhausted: true,
			Detail: `503 {"error":{"type":"server_overloaded","message":"The service is temporarily overloaded"}}`}},
		{Kind: cv.ErrorBlock, Label: b.t("invoice.error.label"), Text: b.t("invoice.error.text")},
	}

	again := b.user(id+"/3", "invoice.3.user")
	logs := b.call(again.ID, "read", "book-open", "logs/backfill-2026-11-08.log")
	logs.Output = output("batch 07: 5000 rows ok\nbatch 08: 5000 rows ok\nbatch 09: ERROR null value in column \"currency\" violates not-null constraint\nbatch 09: rolled back")
	// Commands sent to the background: one still running, one that ended
	// and one that failed, whose results no turn has taken yet.
	backfill := b.call(again.ID, "command", "command", "go run ./cmd/backfill --from-batch 9")
	backfill.Background = &cv.Background{Running: true}
	lint := b.call(again.ID, "command", "command", "make lint")
	lint.Background = &cv.Background{Outcome: cv.OutcomeCompleted, Waiting: true}
	verify := b.call(again.ID, "command", "command", "go test ./internal/billing/...")
	verify.Background = &cv.Background{Outcome: cv.OutcomeFailed, Exit: new(1), Waiting: true}
	again.Reply = done(cv.Continued, 184)
	again.Reply.Blocks = []cv.Block{tools(logs, backfill, lint, verify), text(b.t("invoice.3.answer"))}

	more := b.user(id+"/4", "invoice.4.user")
	more.Reply = done(cv.Completed, 23)
	more.Reply.Blocks = []cv.Block{text(b.t("invoice.4.answer"))}

	only := b.user(id+"/5", "invoice.5.user")
	batch := b.call(only.ID, "command", "command", "go run ./cmd/backfill --year 2024")
	batch.Output = &cv.Output{Text: "batch 01: 5000 rows ok\nbatch 02: 5000 rows ok\nbatch 03: 5000 rows ok", Command: true}
	only.Reply = done(cv.Cancelled, 21)
	only.Reply.Blocks = []cv.Block{tools(batch), text(b.t("invoice.5.answer"))}

	return b.conv(id, "invoice.title", Idle, 4*time.Hour, cv.Usage{Tokens: 96400, Speed: 58.9, Cost: 0.0641, Cached: 71300, Context: 0.48},
		plan, results, why, again, more, only)
}
