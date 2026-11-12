package preview

import (
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
)

// The demo's lumen-web project, an analytics dashboard: a team review that
// waits for its subagents, a design session of questions in every state with
// a plan review, a form and an authorization link, and a report with
// sources, citations, output files, pictures and a chart.

// review is subagent sessions in every status, the second run waiting for
// the ones still at work.
func (b *demo) review() *Conversation {
	const id = "lumen-web/review"
	first := b.user(id+"/0", "review.0.user")
	first.Reply = done(cv.Completed, 372)
	first.Reply.Blocks = []cv.Block{
		{Kind: cv.ChildrenBlock, Children: []cv.Child{
			{ID: first.ID + "/visual", Title: b.t("review.child.visual"), Model: DemoModel, Effort: "high", Status: cv.ChildCompleted, Avatar: "avatar-cat"},
			{ID: first.ID + "/a11y", Title: b.t("review.child.a11y"), Model: demoFast, Effort: "low", Status: cv.ChildFailed},
			{ID: first.ID + "/copy", Title: b.t("review.child.copy"), Model: demoFast, Effort: "low", Status: cv.ChildInterrupted},
			{ID: first.ID + "/legacy", Title: b.t("review.child.legacy"), Model: demoFast, Status: cv.ChildCancelled},
			{ID: first.ID + "/payments", Title: b.t("review.child.payments"), Model: DemoModel, Effort: "medium", Status: cv.ChildContinued},
		}},
		text(b.t("review.0.answer")),
	}
	rerun := b.user(id+"/1", "review.1.user")
	delegate := b.call(rerun.ID, "delegate", "tasks", b.t("review.delegate"))
	delegate.Code, delegate.Standalone = false, true
	rerun.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(94), Phase: cv.WaitingChildren, Waiting: 2}
	rerun.Reply.Blocks = []cv.Block{
		tools(delegate),
		{Kind: cv.ChildrenBlock, Children: []cv.Child{
			{ID: rerun.ID + "/a11y", Title: b.t("review.child.a11y"), Model: DemoModel, Effort: "medium", Status: cv.ChildRunning, Avatar: "avatar-owl"},
			{ID: rerun.ID + "/perf", Title: b.t("review.child.perf"), Model: demoFast, Effort: "low", Status: cv.ChildRunning, Avatar: "avatar-penguin"},
			{ID: rerun.ID + "/copy", Title: b.t("review.child.copy"), Model: demoFast, Effort: "low", Status: cv.ChildStopping},
			{ID: rerun.ID + "/summary", Title: b.t("review.child.summary"), Model: DemoModel, Effort: "medium", Status: cv.ChildQueued},
		}},
	}
	return b.conv(id, "review.title", Running, 5*time.Minute, cv.Usage{Tokens: 128400, Speed: 66.8, Cost: 0.0874, Cached: 98200, Context: 0.62}, first, rerun)
}

// onboarding is the history rows of questions in every state, a plan
// review, a connector's form and its authorization link, and a question
// still open that can be declined.
func (b *demo) onboarding() *Conversation {
	const id = "lumen-web/onboarding"
	options := func(key string) []string { return strings.Split(b.t(key), "\n") }

	first := b.user(id+"/0", "onboard.0.user")
	roles := question(first.ID+"/roles", cv.QuestionAnswered, b.t("onboard.q.roles"), options("onboard.q.roles.options"), options("onboard.q.roles.answer")...)
	roles.Question.Multi = true
	review := cv.Call{ID: first.ID + "/plan", Name: "plan_review", Label: b.t("tool.plan"), Icon: "tasks", Detail: b.t("onboard.plan.detail"),
		Terminal: true, Output: output(b.t("onboard.plan.text"))}
	first.Reply = done(cv.Completed, 184)
	first.Reply.Blocks = []cv.Block{
		question(first.ID+"/first", cv.QuestionAnswered, b.t("onboard.q.first"), options("onboard.q.first.options"), b.t("onboard.q.first.answer")),
		roles,
		question(first.ID+"/usage", cv.QuestionDeclined, b.t("onboard.q.usage"), nil),
		question(first.ID+"/accent", cv.QuestionCancelled, b.t("onboard.q.accent"), options("onboard.q.accent.options")),
		tools(review),
		text(b.t("onboard.0.answer")),
	}

	issues := b.user(id+"/1", "onboard.1.user")
	form := cv.Call{ID: issues.ID + "/form", Name: "mcp_elicitation", Label: "Trackboard", Icon: "tasks", Detail: b.t("onboard.form.detail"),
		Terminal: true, Standalone: true, Output: output(b.t("onboard.form.text"))}
	link := cv.Call{ID: issues.ID + "/link", Name: "mcp_elicitation", Label: "Pulse Analytics", Icon: "globe", Detail: "https://auth.pulse.example/device",
		Terminal: true, Standalone: true}
	issues.Reply = done(cv.Completed, 246)
	issues.Reply.Blocks = []cv.Block{
		tools(form),
		question(issues.ID+"/team", cv.QuestionUnconsumed, b.t("onboard.q.team"), nil, "Growth"),
		tools(link),
		question(issues.ID+"/code", cv.QuestionInterrupted, b.t("onboard.q.code"), nil),
		question(issues.ID+"/workspace", cv.QuestionTimedOut, b.t("onboard.q.workspace"), []string{"lumen-production", "lumen-staging"}),
		question(issues.ID+"/digest", cv.QuestionAccepted, b.t("onboard.q.digest"), options("onboard.q.digest.options"), b.t("onboard.q.digest.answer")),
		text(b.t("onboard.1.answer")),
	}

	tone := b.user(id+"/2", "onboard.2.user")
	tone.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(38), Phase: cv.Generating}
	tone.Reply.Blocks = []cv.Block{
		reasoning(b.t("onboard.2.think")),
		question(tone.ID+"/ask", cv.QuestionPending, b.t("onboard.ask.prompt"), nil),
	}
	c := b.conv(id, "onboard.title", Waiting, 12*time.Minute, cv.Usage{Tokens: 22600, Speed: 69.1, Cost: 0.0139, Cached: 15800, Context: 0.11}, first, issues, tone)
	c.Waiting = composer.Interactions{Question: &composer.Ask{ID: tone.ID + "/ask", Prompt: b.t("onboard.ask.prompt"),
		Options: options("onboard.ask.options"), Multi: true, Declinable: true}}
	return c
}

const retentionSQL = "SELECT cohort_week,\n       count(DISTINCT team_id) FILTER (WHERE active_week = cohort_week + 4) * 1.0\n         / count(DISTINCT team_id) AS week_4_retention\nFROM team_activity\nGROUP BY cohort_week\nORDER BY cohort_week DESC\nLIMIT 8;\n"

const eventsCSV = "day,signups,activated,dashboards\n2026-11-02,412,268,731\n2026-11-03,455,301,804\n2026-11-04,438,289,792\n2026-11-05,471,322,866\n2026-11-06,502,347,915\n"

const teamsChart = "xychart-beta\n    title \"Weekly active teams\"\n    x-axis [W40, W41, W42, W43, W44, W45]\n    y-axis \"teams\" 0 --> 2400\n    bar [1420, 1510, 1630, 1690, 1840, 2015]\n"

// retentionChart is the second chart of the report: a line on a range of
// its own.
const retentionChart = "xychart-beta\n    title \"Week-4 retention\"\n    x-axis [W40, W41, W42, W43, W44, W45]\n    y-axis \"%\" 40 --> 70\n    line [52, 53, 55, 54, 56, 58]\n"

const boardHTML = "<!doctype html>\n<title>Weekly metrics</title>\n<main>\n  <h1>Week 45</h1>\n  <label>Plan <select id=\"plan\"><option>All</option><option>Free</option><option>Pro</option></select></label>\n  <canvas id=\"teams\" width=\"640\" height=\"280\"></canvas>\n</main>\n<script type=\"module\" src=\"./board.js\"></script>\n"

// metrics is an answer grounded on a search with its citations and the
// search's own suggestions, output files of every kind and preview state
// with a published document and a published page, pictures the model made
// and pictures a tool returned, two charts, links that show as a file card and
// as pictures, and Markdown with math, a diagram and links.
func (b *demo) metrics() *Conversation {
	const id = "lumen-web/metrics"
	pics := pictures()
	hero := cv.Image{ID: "demo-hero-growth", Name: "hero-growth.png", Width: pics[0].Width, Height: pics[0].Height, Source: pics[0].Source}
	cover := cv.Image{ID: "demo-hero-retention", Name: "hero-retention.png", Width: pics[2].Width, Height: pics[2].Height, Source: pics[2].Source}
	chart := demoChart()
	chart.ID, chart.Name = "demo-retention-chart", "retention.png"

	report := b.user(id+"/0", "metrics.0.user")
	answer := b.t("metrics.0.answer")
	// The first source is cited where the opening paragraph ends, the second
	// after the answer.
	end := utf8.RuneCountInString(answer)
	if i := strings.Index(answer, "\n"); i >= 0 {
		end = utf8.RuneCountInString(answer[:i])
	}
	benchmark := cv.Citation{URI: "https://lumen.example/blog/b2b-activation-benchmarks", Title: b.t("metrics.source.benchmark")}
	guide := cv.Citation{URI: "https://lumen.example/docs/metrics/retention", Title: b.t("metrics.source.guide")}
	write := b.call(report.ID, "write", "writing", "reports/metrics-week-45.md")
	report.Reply = done(cv.Completed, 127)
	report.Reply.Blocks = []cv.Block{
		{Kind: cv.SearchBlock, Sources: []cv.Citation{benchmark, guide}, Suggestions: []cv.Citation{
			{URI: "https://search.lumen.example/?q=b2b+activation+rate+benchmark", Title: b.t("metrics.suggest.activation")},
			{URI: "https://search.lumen.example/?q=week+4+retention+saas", Title: b.t("metrics.suggest.retention")},
		}},
		tools(write),
		text(answer),
		{Kind: cv.FilesBlock, Files: []cv.File{
			{ID: report.ID + "/report", Path: "reports/metrics-week-45.md", Mime: "text/markdown", Size: 6 << 10, Kind: cv.MarkdownFile, Text: b.t("metrics.report"), Editable: true, Document: true, Exportable: true},
			{ID: report.ID + "/board", Path: "exports/metrics-board.html", Mime: "text/html", Size: 1 << 10, Kind: cv.CodeFile, Text: boardHTML, Document: true, Exportable: true, Interactive: true},
			{ID: report.ID + "/sql", Path: "queries/retention.sql", Mime: "application/sql", Size: 1 << 10, Kind: cv.CodeFile, Text: retentionSQL, Editable: true},
			{ID: report.ID + "/csv", Path: "data/events-week-45.csv", Mime: "text/csv", Size: 2 << 10, Kind: cv.TextFile, Text: eventsCSV},
			{ID: report.ID + "/chart", Path: "charts/retention.png", Mime: "image/png", Size: 96 << 10, Kind: cv.ImageFile, Image: &chart},
			{ID: report.ID + "/pdf", Path: "exports/metrics-week-45.pdf", Mime: "application/pdf", Size: 480 << 10, Kind: cv.OtherFile},
			{ID: report.ID + "/raw", Path: "data/raw-events.json", Mime: "application/json", Size: 64 << 20, Kind: cv.TextFile, State: cv.PreviewLarge},
			{ID: report.ID + "/funnel", Path: "dashboards/funnel.json", Mime: "application/json", Size: 4 << 10, Kind: cv.CodeFile, State: cv.PreviewChanged},
			{ID: report.ID + "/log", Path: "logs/export.log", Mime: "text/plain", Size: 3 << 10, Kind: cv.TextFile, State: cv.PreviewFailed},
			{ID: report.ID + "/segments", Path: "data/segments.csv", Mime: "text/csv", Size: 900 << 10, Kind: cv.TextFile, State: cv.PreviewLoading},
		}},
		{Kind: cv.ImagesBlock, Images: []cv.Image{hero, cover}},
		{Kind: cv.VisualBlock, Visual: &cv.Visual{Title: b.t("metrics.chart.title"), Mermaid: teamsChart}},
		{Kind: cv.VisualBlock, Visual: &cv.Visual{Title: b.t("metrics.retention.title"), Mermaid: retentionChart}},
	}
	benchmark.End = end
	report.Reply.Citations = []cv.Citation{benchmark, guide}

	formula := b.user(id+"/1", "metrics.1.user")
	formula.Reply = done(cv.Completed, 19)
	formula.Reply.Blocks = []cv.Block{reasoning(b.t("metrics.1.think")), text(b.t("metrics.1.answer"))}

	// The pictures a tool returned, then a reply whose links stand alone:
	// the tool's picture by its reference, one that is gone, a picture of
	// the workspace and a file's card (Embed).
	look := b.user(id+"/2", "metrics.2.user")
	shot := cv.Call{ID: look.ID + "/shot", Name: "screenshot", Label: b.t("tool.screenshot"), Icon: "globe", Detail: "https://app.lumen.example/dashboards/weekly", Code: true}
	shot.Output = &cv.Output{Images: []cv.Image{
		{ID: "demo-shot-dashboard", Name: "dashboard.png", Width: pics[1].Width, Height: pics[1].Height, Source: pics[1].Source, Reference: shot.ID + "/0"},
		{ID: "demo-shot-mobile", Name: "dashboard-mobile.png", Missing: true, Reference: shot.ID + "/1"},
	}}
	look.Reply = done(cv.Completed, 34)
	look.Reply.Blocks = []cv.Block{tools(shot), text(b.t("metrics.2.answer") +
		"\n\n![dashboard.png](sailry-image:" + shot.ID + "/0)" +
		"\n\n![dashboard-mobile.png](sailry-image:" + shot.ID + "/1)" +
		"\n\n![retention.png](charts/retention.png)" +
		"\n\n[metrics-week-45.md](reports/metrics-week-45.md)")}
	// The report is the last turn: the session overview lists its sources.
	return b.conv(id, "metrics.title", Unread, time.Hour, cv.Usage{Tokens: 41900, Speed: 73.5, Cost: 0.0267, Cached: 30200, Context: 0.21}, formula, look, report)
}
