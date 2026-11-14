package preview

import (
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	cv "github.com/sailry/sailry-harness/internal/desktop/conversation"
)

// The demo's chats without a project: Markdown of every kind, context
// compactions in every state, and the session states around a transcript:
// older pages that did not load, a connection recovering with a diagnostic,
// a host that is offline, and runs that are stopping and waiting to start.

// hashing is an answer in Markdown of every kind: headings, lists, a table,
// code, a quote, math, a diagram and links.
func (b *demo) hashing() *Conversation {
	const id = "chat/hashing"
	explain := b.user(id+"/0", "hash.0.user")
	explain.Reply = done(cv.Completed, 18)
	explain.Reply.Blocks = []cv.Block{reasoning(b.t("hash.0.think")), text(b.t("hash.0.answer"))}
	lookup := b.user(id+"/1", "hash.1.user")
	lookup.Reply = done(cv.Completed, 11)
	lookup.Reply.Blocks = []cv.Block{text(b.t("hash.1.answer"))}
	// A chat has no project to make a worktree in; its last message is
	// open in its editor.
	explain.Controls = cv.Controls{NoWorktree: true}
	lookup.Controls, lookup.Editing = cv.Controls{NoWorktree: true}, &cv.Editing{Text: lookup.User}
	c := b.conv(id, "hash.title", Idle, 2*time.Hour, cv.Usage{Tokens: 6800, Speed: 78.2, Cost: 0.0041, Cached: 3900, Context: 0.03}, explain, lookup)
	order := 0
	c.Pinned, c.PinOrder = true, &order
	return c
}

// handbook is a long conversation's compactions: a handoff summary inside a
// reply, then compaction turns in every state with how long they took, the
// last ones still running and waiting.
func (b *demo) handbook() *Conversation {
	const id = "chat/handbook"
	compaction := func(n string, k cv.Compaction, st cv.RunStatus) cv.Turn {
		return cv.Turn{ID: id + "/" + n, Compaction: &k, Reply: cv.Reply{Status: st}}
	}
	start := b.user(id+"/0", "handbook.0.user")
	chapters := b.call(start.ID, "read", "book-open", "handbook/01-principles.md")
	chapters.Output = output(b.t("handbook.file"))
	// A built-in tool is labeled by the application's own catalog.
	skill := cv.Call{ID: start.ID + "/skill", Name: "load_skill", LabelKey: "tool_load_skill", Icon: "book-open", Detail: "technical-summaries"}
	start.Reply = done(cv.Completed, 204)
	start.Reply.Blocks = []cv.Block{
		tools(skill, chapters),
		text(b.t("handbook.0.answer")),
		{Kind: cv.CompactionBlock, Label: "handoff", Text: b.t("handbook.handoff")},
		text(b.t("handbook.0.more")),
	}
	ask := b.user(id+"/2", "handbook.2.user")
	ask.Reply = done(cv.Completed, 14)
	// A long chapter's answer ended at the model's output limit.
	ask.Reply.Blocks = []cv.Block{text(b.t("handbook.2.answer")), {Kind: cv.LimitBlock}}
	next := b.user(id+"/6", "handbook.6.user")
	next.Reply = cv.Reply{Status: cv.Running, Elapsed: secs(47), Phase: cv.Compacting, PhaseElapsed: secs(12)}
	next.Reply.Blocks = []cv.Block{text(b.t("handbook.6.partial"))}
	running := compaction("7", cv.Compaction{State: cv.CompactionRunning}, cv.Running)
	running.Reply.Phase = cv.Compacting
	return b.conv(id, "handbook.title", Running, 9*time.Minute, cv.Usage{Tokens: 184600, Speed: 61.3, Cost: 0.1218, Cached: 152300, Context: 0.91},
		start,
		compaction("1", cv.Compaction{State: cv.Compacted, Summary: b.t("handbook.summary"), Elapsed: secs(38)}, cv.Completed),
		ask,
		compaction("3", cv.Compaction{State: cv.CompactionUnneeded}, cv.Completed),
		compaction("4", cv.Compaction{State: cv.CompactionFailed, Cause: b.t("handbook.cause")}, cv.Completed),
		compaction("5", cv.Compaction{State: cv.CompactionCancelled}, cv.Completed),
		compaction("5/interrupted", cv.Compaction{State: cv.CompactionInterrupted, Elapsed: secs(83)}, cv.Completed),
		next,
		running,
		compaction("8", cv.Compaction{State: cv.CompactionQueued}, cv.Queued),
	)
}

// staging is a session whose host is reconnecting: a diagnostic of what
// failed, a run and compaction whose outcomes are not synced, a delegated
// task, a question that cannot be answered, and an older page that did not load.
func (b *demo) staging() *Conversation {
	const id = "chat/staging"
	health := b.user(id+"/0", "staging.0.user")
	probe := b.call(health.ID, "command", "command", "curl -s https://staging.harbor.example/healthz")
	probe.Output = &cv.Output{Text: `{"status":"ok","version":"2.4.0-rc.2","db":"ok","queue":"ok"}`, Command: true}
	health.Reply = done(cv.Completed, 9)
	health.Reply.Blocks = []cv.Block{tools(probe), text(b.t("staging.0.answer"))}
	tail := b.user(id+"/1", "staging.1.user")
	logs := b.call(tail.ID, "command", "command", "kubectl logs deploy/harbor-api --since=10m | grep ERROR")
	tail.Reply = cv.Reply{Status: cv.Unsynced, Phase: cv.AwaitingResponse}
	tail.Reply.Blocks = []cv.Block{
		tools(logs),
		{Kind: cv.ChildrenBlock, Children: []cv.Child{{ID: tail.ID + "/logs", Title: b.t("staging.1.user"), Model: demoFast, Status: cv.ChildRunning, Unsynced: true}}},
		question(tail.ID+"/ask", cv.QuestionPending, b.t("staging.ask.prompt"), nil),
	}
	compact := cv.Turn{ID: id + "/compact", Compaction: &cv.Compaction{State: cv.CompactionUnsynced}, Reply: cv.Reply{Status: cv.Unsynced}}
	// Nothing is exported or answered while the host does not answer.
	health.Controls = cv.Controls{NoExport: true, NoWorktree: true}
	tail.Controls = cv.Controls{NoExport: true, NoWorktree: true, NoEdit: true, NoFork: true, NoRewind: true}
	c := b.conv(id, "staging.title", Waiting, 7*time.Minute, cv.Usage{Tokens: 9200, Speed: 55.0, Cost: 0.0058, Cached: 6100, Context: 0.05}, health, tail, compact)
	c.Waiting.Question = &composer.Ask{ID: tail.ID + "/ask", Prompt: b.t("staging.ask.prompt"),
		Options: strings.Split(b.t("staging.ask.options"), "\n"), Disabled: true}
	c.Session.Older, c.Session.OlderFailed = 12, true
	c.Session.Connection = cv.Reconnecting
	c.Session.Diagnostic = &cv.Diagnostic{Title: b.t("staging.diagnostic"), Fields: [][2]string{
		{"host", "staging-2"}, {"status", "503"}, {"type", "upstream_unavailable"}, {"request", "req_8f41c2…"},
	}, Detail: `{"error":{"type":"upstream_unavailable","message":"no healthy upstream","retry_after":15}}`}
	return c
}

// releaseNotes is a run that is stopping while the model still thinks.
func (b *demo) releaseNotes() *Conversation {
	const id = "chat/release-notes"
	draft := b.user(id+"/0", "release.0.user")
	draft.Reply = done(cv.Completed, 27)
	draft.Reply.Blocks = []cv.Block{text(b.t("release.0.answer"))}
	shorter := b.user(id+"/1", "release.1.user")
	shorter.Reply = cv.Reply{Status: cv.Stopping, Elapsed: secs(6), Phase: cv.Thinking}
	shorter.Reply.Blocks = []cv.Block{reasoning(b.t("release.1.think"))}
	return b.conv(id, "release.title", Running, 16*time.Minute, cv.Usage{Tokens: 5400, Speed: 80.6, Cost: 0.0033, Cached: 2800, Context: 0.02}, draft, shorter)
}

// translate is a finished chat whose host went offline.
func (b *demo) translate() *Conversation {
	const id = "chat/translate"
	ask := b.user(id+"/0", "translate.0.user")
	ask.Reply = done(cv.Completed, 8)
	ask.Reply.Blocks = []cv.Block{text(b.t("translate.0.answer"))}
	c := b.conv(id, "translate.title", Idle, 26*time.Hour, cv.Usage{Tokens: 2100, Speed: 82.4, Cost: 0.0012, Cached: 900, Context: 0.01}, ask)
	c.Session.Connection = cv.Offline
	c.Turns[0].Controls = cv.Controls{NoExport: true, NoWorktree: true}
	return c
}
