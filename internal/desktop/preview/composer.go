package preview

import (
	"fmt"
	"math"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// The composer's preview data: the model picker's families, scores and
// features, the teams, the @ and / catalogs,
// the queue and the simulated transfers and dictation. Nothing here reads
// a file, asks a model or records audio: transfers and voice input move
// on the clock alone.

// Families are the model picker's rail (settings/providers families).
var Families = []composer.Family{
	{ID: "anthropic", Label: "provider_anthropic", Icon: icons.Brand("anthropic")},
	{ID: "openai", Label: "provider_openai", Icon: icons.Brand("openai")},
	{ID: "gemini", Label: "provider_gemini", Icon: icons.Brand("gemini")},
	{ID: "other", Label: "provider_other", Icon: icons.Settings2},
}

// Teams are the preview's teams (teams settings).
var Teams = []composer.Team{
	{ID: "review", Name: "Review crew", Members: []composer.Member{{ID: "reader", Name: "Reader", Avatar: "avatar-rabbit"}, {ID: "critic", Name: "Critic", Overridden: true, Avatar: "avatar-owl"}}},
	{ID: "ship", Name: "Ship it", Members: []composer.Member{{ID: "builder", Name: "Builder", Avatar: "avatar-bear"}, {ID: "tester", Name: "Tester"}, {ID: "writer", Name: "Writer", Avatar: "avatar-penguin"}}},
}

// models are the preview's models: families, made-up scores and the
// speed feature where a provider offers it.
func models() []Model {
	fast := []composer.Feature{{ID: "speed", Name: "Speed", Options: []composer.FeatureOption{{ID: "fast", Name: "Fast"}}}}
	verbosity := composer.Feature{ID: "verbosity", Name: "Verbosity", Icon: icons.Quote, Options: []composer.FeatureOption{
		{ID: "low", Name: "Low"}, {ID: "high", Name: "High"}}}
	score := func(overall float64, tested string, axes ...float64) *composer.Score {
		s := &composer.Score{Overall: overall, Tested: tested}
		copy(s.Axes[:], axes)
		return s
	}
	return []Model{
		{ID: "claude-opus-5-5", Name: "Claude Opus 5.5", Provider: "Anthropic", Reasoning: true, Family: "anthropic",
			Score: score(9.1, "high", 9.3, 8.8, 9.4, 8.6, 9.2), Features: fast},
		{ID: "claude-sonnet-5-5", Name: "Claude Sonnet 5.5", Provider: "Anthropic", Reasoning: true, Family: "anthropic",
			Score: score(8.6, "medium", 8.8, 8.5, 8.7, 8.2, 8.6)},
		{ID: "claude-haiku-5-5", Name: "Claude Haiku 5.5", Provider: "Anthropic", Family: "anthropic"},
		{ID: "gpt-5.2", Name: "GPT-5.2", Provider: "OpenAI", Reasoning: true, Family: "openai",
			Score: score(8.9, "high", 9.0, 8.4, 9.3, 8.1, 8.8), Features: append(slices.Clone(fast), verbosity)},
		{ID: "gemini-3-pro", Name: "Gemini 3 Pro", Provider: "Google", Reasoning: true, Family: "gemini",
			Efforts: []string{"low", "high"}},
		{ID: "qwen3-coder", Name: "Qwen3 Coder", Provider: "Ollama", Family: "other"},
	}
}

// Composer is what the composer of conversation c is offered: the data's
// models and teams, the hosts and projects, its worktree's inspection,
// the catalogs and the file picker's folder.
func Composer(d *Data, c *Conversation, worktree string) *composer.Context {
	ctx := &composer.Context{
		Models: d.Models, Families: Families, Teams: Teams, Interactions: c.Waiting, Queue: c.Queue,
	}
	for _, h := range d.Hosts {
		ctx.Hosts = append(ctx.Hosts, composer.Host{ID: h.ID, Name: h.Name, Local: h.Local})
	}
	for _, p := range d.Projects {
		pr := composer.Project{ID: p.ID, Name: p.Name, Host: p.Host}
		for _, w := range p.Worktrees {
			pr.Worktrees = append(pr.Worktrees, composer.Worktree{Branch: w.Branch, Main: w.Main})
		}
		ctx.Projects = append(ctx.Projects, pr)
	}
	ctx.Repository = composer.Repository{Kind: composer.Git, Branch: worktree}
	if p := d.ProjectOf(c.ID); p != nil && p.ID == "notes" {
		ctx.Repository = composer.Repository{Kind: composer.Directory} // field-notes is a plain folder
	}
	for i := len(c.Turns) - 1; i >= 0; i-- {
		if t := c.Turns[i]; t.User != "" {
			if len(t.Images) == 0 && len(t.Files) == 0 {
				ctx.Last = t.User
			}
			break
		}
	}
	ctx.References, ctx.Commands = references(d, c), commands(d.Models)
	ctx.Activity = activity(d, c, ctx.Repository)
	return ctx
}

// references is the @ catalog: files, subagents, sessions, the workspace
// context, databases, SSH, the skills and plugins, then attaching files.
func references(d *Data, c *Conversation) []composer.Reference {
	page := func(id, label, desc, alias string, icon *ui.SVG, kids ...composer.Reference) composer.Reference {
		return composer.Reference{ID: id, Kind: composer.Page, Label: label, Description: desc, Key: true, Alias: alias,
			Icon: icon, Children: kids}
	}
	leaf := func(kind composer.ReferenceKind, id, label, desc string, icon *ui.SVG) composer.Reference {
		return composer.Reference{ID: id, Kind: kind, Label: label, Description: desc, Icon: icon}
	}
	var sessions []composer.Reference
	if p := d.ProjectOf(c.ID); p != nil {
		for _, s := range p.Conversations {
			if s.ID != c.ID {
				sessions = append(sessions, leaf(composer.Session, "session:"+s.ID, s.Title, p.Name, icons.Chat))
			}
		}
	}
	project, branch := "", ""
	if p := d.ProjectOf(c.ID); p != nil {
		project, branch = p.Name, p.Branch
	}
	context := []composer.Reference{{ID: "host", Kind: composer.Location, Label: "local_host_name", Description: "reference_host",
		Key: true, Icon: icons.Cpu}}
	if project != "" {
		context = append(context, leaf(composer.Location, "project", project, "", icons.Folder),
			leaf(composer.Location, "worktree", branch, "", icons.GitBranch))
	}
	tables := func(db string, names ...string) []composer.Reference {
		var out []composer.Reference
		for _, n := range names {
			out = append(out, leaf(composer.Database, "db:"+db+"/"+n, n, db, icons.Grid))
		}
		return out
	}
	databases := []composer.Reference{
		{ID: "db:billing", Kind: composer.Page, Label: "billing (PostgreSQL)", Description: "localhost:5432", Icon: icons.MemoryStick,
			Children: tables("billing", "invoices", "customers", "tax_rates")},
		{ID: "db:cache", Kind: composer.Page, Label: "cache (SQLite)", Description: "~/.cache/app.db", Icon: icons.MemoryStick,
			Children: tables("cache", "entries")},
	}
	var catalog []composer.Reference
	for _, p := range d.Plugins {
		if p.Enabled {
			catalog = append(catalog, leaf(composer.Plugin, "plugin:"+p.ID, p.Name, p.Description, icons.Puzzle))
		}
	}
	group := func(key string, list ...composer.Reference) []composer.Reference {
		for i := range list {
			list[i].Group = key
		}
		return list
	}
	for i := range catalog {
		catalog[i].Group = "reference_group_plugins"
	}
	out := group("reference_group_context",
		page("files", "reference_files", "reference_files_description", "files", icons.Folder, fileRefs(d.Files)...),
		page("agents", "reference_agents", "reference_agents_description", "agents", icons.Bot,
			leaf(composer.Agent, "agent:reviewer", "@reviewer", "Reviews diffs for bugs", icons.Bot),
			leaf(composer.Agent, "agent:tester", "@tester", "Writes and runs tests", icons.Bot)),
		page("sessions", "reference_sessions", "reference_sessions_description", "sessions", icons.Chat, sessions...),
		page("context", "reference_context", "reference_context_description", "context", icons.Globe, context...))
	out = append(out, group("reference_group_connections",
		page("databases", "reference_databases", "", "database", icons.MemoryStick, databases...),
		page("ssh", "reference_ssh", "", "ssh", icons.Terminal,
			leaf(composer.Location, "ssh:build", "build-box", "ci@10.0.0.12", icons.Terminal)))...)
	out = append(out, group("reference_group_skills",
		leaf(composer.Skill, "skill:git/commit", "Commit message", "git · Write a commit message from the diff", icons.BookOpen),
		leaf(composer.Skill, "skill:knowledge/search", "Search knowledge", "knowledge · Search local documents", icons.BookOpen))...)
	out = append(out, catalog...)
	return append(out, composer.Reference{ID: "attach", Kind: composer.AttachFiles, Label: "composer_attachment", Key: true,
		Icon: icons.Plus, Alias: "attachment upload", Group: "reference_group_attach"})
}

// fileRefs are the worktree's files as reference pages (Page::Files): a
// folder opens to its entries, with Select current first.
func fileRefs(n *FileNode) []composer.Reference {
	var out []composer.Reference
	for _, k := range n.Children {
		rel := k.Path[strings.IndexByte(k.Path, '/')+1:] // without the root folder
		if k.Children != nil {
			out = append(out, composer.Reference{ID: "dir:" + rel, Kind: composer.Folder, Label: rel, Icon: icons.Folder,
				Children: fileRefs(k)})
			continue
		}
		out = append(out, composer.Reference{ID: "file:" + rel, Kind: composer.File, Label: rel, Icon: icons.FileText})
	}
	return out
}

// commands is the / catalog: the prompts, the modes, /model and /reasoning
// pages and /compact.
func commands(list []Model) []composer.Reference {
	var out []composer.Reference
	for _, name := range []string{"review", "test", "plan", "explain"} {
		icon := map[string]*ui.SVG{"review": icons.Eye, "test": icons.Terminal, "plan": icons.Dashboard, "explain": icons.Info}[name]
		out = append(out, composer.Reference{ID: "prompt:" + name, Kind: composer.Prompt, Label: "/" + name, Key: true,
			Description: "composer_command_" + name, Icon: icon, Insert: "composer_prompt_" + name, Alias: name, Group: "composer_group_prompts"})
	}
	for _, m := range []string{"code", "plan", "flagship"} {
		out = append(out, composer.Reference{ID: "mode:" + m, Kind: composer.ModeCommand, Label: "/" + m,
			Description: "composer_mode_" + m, Key: true, Icon: icons.Dashboard, Insert: "composer_mode_" + m, Group: "composer_group_modes"})
	}
	var models []composer.Reference
	for _, m := range list {
		models = append(models, composer.Reference{ID: "model:" + m.ID, Kind: composer.ModelCommand, Label: m.Name,
			Description: m.Provider, Icon: icons.Bot, Insert: m.ID})
	}
	var efforts []composer.Reference
	for _, e := range composer.Efforts {
		efforts = append(efforts, composer.Reference{ID: "effort:" + e, Kind: composer.EffortCommand, Label: e, Icon: icons.Bot, Insert: e})
	}
	return append(out,
		composer.Reference{ID: "models", Kind: composer.Page, Label: "/model", Description: "composer_select_model", Key: true, Icon: icons.Bot,
			Alias: "model", Children: models, Group: "composer_group_session"},
		composer.Reference{ID: "reasoning", Kind: composer.Page, Label: "/reasoning", Description: "composer_effort", Key: true, Icon: icons.Bot,
			Alias: "reasoning", Children: efforts, Group: "composer_group_session"},
		composer.Reference{ID: "compact", Kind: composer.Compact, Label: "/compact", Description: "chat_compact", Key: true, Icon: icons.Minimize, Group: "composer_group_session"})
}

// Queue puts a message behind the running reply, or sends it at once
// when nothing runs.
func Queue(c *Conversation, m composer.Message, now time.Time) {
	if !c.Running() {
		Send(c, m.Text, now)
		return
	}
	simulation(c).queued++
	c.Queue = append(c.Queue, composer.Queued{ID: fmt.Sprintf("%s/queued-%d", c.ID, simulation(c).queued), Text: m.Text,
		Attachments: slices.Clone(m.Attachments)})
}

// Guide joins a message to the running reply (GuideTurn): it waits in the
// queue with its guidance pending until withdrawn or the reply ends, as
// the preview's run takes no guidance. It sends at once when nothing
// runs.
func Guide(c *Conversation, m composer.Message, now time.Time) {
	Queue(c, m, now)
	if c.Running() {
		c.Queue[len(c.Queue)-1].Guided = true
	}
}

// SendQueued sends a queued message now: while a reply runs, a message
// guides it (GuideQueuedTurn) and a compaction goes first in the queue
// (SendQueuedTurn); otherwise it is sent.
func SendQueued(c *Conversation, id string, now time.Time) {
	i := slices.IndexFunc(c.Queue, func(q composer.Queued) bool { return q.ID == id })
	if i < 0 || c.Queue[i].Guided {
		return
	}
	q := c.Queue[i]
	switch {
	case c.Running() && q.Compaction:
		MoveQueued(c, id, c.Queue[0].ID)
	case c.Running():
		c.Queue[i].Guided = true
	default:
		c.Queue = slices.Delete(c.Queue, i, i+1)
		Send(c, q.Text, now)
	}
}

// WithdrawGuidance returns a guiding message to the queue
// (WithdrawGuidance).
func WithdrawGuidance(c *Conversation, id string) {
	for i := range c.Queue {
		if c.Queue[i].ID == id {
			c.Queue[i].Guided = false
		}
	}
}

// SaveQueued replaces a queued message's text.
func SaveQueued(c *Conversation, id, text string) {
	for i := range c.Queue {
		if c.Queue[i].ID == id {
			c.Queue[i].Text = text
		}
	}
}

// RemoveQueued takes a message out of the queue.
func RemoveQueued(c *Conversation, id string) {
	c.Queue = slices.DeleteFunc(c.Queue, func(q composer.Queued) bool { return q.ID == id })
}

// MoveQueued moves a queued message before another, or to the end.
func MoveQueued(c *Conversation, id, before string) {
	i := slices.IndexFunc(c.Queue, func(q composer.Queued) bool { return q.ID == id })
	if i < 0 || id == before {
		return
	}
	q := c.Queue[i]
	c.Queue = slices.Delete(c.Queue, i, i+1)
	j := slices.IndexFunc(c.Queue, func(q composer.Queued) bool { return q.ID == before })
	if j < 0 {
		j = len(c.Queue)
	}
	c.Queue = slices.Insert(c.Queue, j, q)
}

// Transfers move the draft's files on the clock: preparing for 200 ms,
// sending over a second and a half, publishing for 300 ms. A file named
// with "fail" fails, so the failed card and Retry show.
type Transfers struct {
	started map[*composer.Attachment]time.Time
}

// Start (re)starts a file's transfer at now.
func (t *Transfers) Start(a *composer.Attachment, now time.Time) {
	if t.started == nil {
		t.started = map[*composer.Attachment]time.Time{}
	}
	t.started[a] = now
	a.Status, a.Progress, a.Error = composer.Preparing, 0, ""
}

// Drop forgets a file taken out of the draft.
func (t *Transfers) Drop(a *composer.Attachment) { delete(t.started, a) }

// Advance moves the transfers to now and reports whether one still runs.
func (t *Transfers) Advance(list []*composer.Attachment, now time.Time) bool {
	running := false
	for _, a := range list {
		at, ok := t.started[a]
		if !ok {
			continue
		}
		d := now.Sub(at)
		switch {
		case d < 200*time.Millisecond:
			a.Status = composer.Preparing
		case d < 1700*time.Millisecond:
			a.Status, a.Progress = composer.Sending, float32(d-200*time.Millisecond)/float32(1500*time.Millisecond)
			if strings.Contains(a.Name, "fail") && a.Progress > 0.6 {
				a.Status, a.Error = composer.Failed, "chat_attachment_unknown"
				delete(t.started, a)
				continue
			}
		case d < 2000*time.Millisecond:
			a.Status = composer.Publishing
		default:
			a.Status, a.Progress = composer.Ready, 1
			delete(t.started, a)
			continue
		}
		running = true
	}
	return running
}

// Voice simulates voice input: preparing for 400 ms, then recording with
// levels of a made-up wave, transcribing for 800 ms after Finish. No
// microphone is opened and nothing is transcribed.
type Voice struct {
	since time.Time
}

// Command moves d by the button's command at now.
func (v *Voice) Command(d *composer.Dictation, cmd composer.DictationCommand, now time.Time) {
	v.since = now
	switch cmd {
	case composer.StartDictation:
		d.Phase, d.Levels = composer.PreparingSpeech, nil
	case composer.FinishDictation:
		d.Phase = composer.Transcribing
	default:
		d.Phase, d.Levels = composer.Idle, nil
	}
}

// Advance moves d to now, reporting whether it still changes and whether
// a transcription just ended.
func (v *Voice) Advance(d *composer.Dictation, now time.Time) (changing, done bool) {
	t := now.Sub(v.since)
	switch d.Phase {
	case composer.PreparingSpeech:
		if t >= 400*time.Millisecond {
			d.Phase, v.since = composer.Recording, now
		}
		return true, false
	case composer.Recording:
		x := t.Seconds()
		level := float32(0.5 + 0.45*math.Sin(x*9)*math.Sin(x*2.3))
		d.Levels = append(d.Levels, level)
		if len(d.Levels) > 8 {
			d.Levels = d.Levels[len(d.Levels)-8:]
		}
		return true, false
	case composer.Transcribing:
		if t >= 800*time.Millisecond {
			d.Phase, d.Levels = composer.Idle, nil
			return false, true
		}
		return true, false
	}
	return false, false
}

// activity is the strip over c's composer: the worktree's Git changes
// (the Git page's), the plugins' badges, the running turn's progress and
// subagents, and the background commands.
func activity(d *Data, c *Conversation, repo composer.Repository) composer.Activity {
	a := composer.Activity{Badges: c.Badges, Commands: c.Commands, Connected: c.Session.Connection == conversation.Connected}
	if repo.Kind == composer.Git && d.ProjectOf(c.ID) != nil {
		ch := &composer.Changes{Files: len(d.Workspace.Changes)}
		for _, f := range d.Workspace.Changes {
			ch.Added += f.Added
			ch.Removed += f.Removed
			// The preview's changes are in the order they were made last.
			if len(ch.Recent) < composer.RecentChanges {
				ch.Recent = append(ch.Recent, composer.ChangedFile{Path: f.Path, Status: f.Status, Added: f.Added, Removed: f.Removed})
			}
		}
		a.Changes = ch
	}
	if !c.Running() {
		return a
	}
	for _, b := range c.Turns[len(c.Turns)-1].Reply.Blocks {
		switch b.Kind {
		case conversation.ProgressBlock:
			a.Progress = b.Steps
		case conversation.ChildrenBlock:
			for _, k := range b.Children {
				if k.Status == conversation.ChildRunning {
					a.Children = append(a.Children, k)
				}
			}
		}
	}
	return a
}

// background is the stress conversation's background commands: a
// benchmark, and a preview server exposing a port.
func background() []composer.Command {
	start := time.Now()
	return []composer.Command{
		{ID: "cmd-bench", Command: "go test ./internal/conversation -run '^$' -bench .", Started: start.Add(-95 * time.Second),
			Output: "goos: darwin\ngoarch: arm64\nBenchmarkScroll-14    1785    670849 ns/op"},
		{ID: "cmd-serve", Command: "npx vite --port 5173", Started: start.Add(-12 * time.Minute), Ports: []int{5173},
			Output: "VITE v6.0.0  ready in 312 ms\n\n➜  Local:   http://localhost:5173/"},
	}
}

// goal is the goals plugin's Status badge as the demo's goals have it
// (goalStatus): Running · 12m, the counters as its tooltip, and the panel
// with the objective, Pause and End.
func goal() composer.StatusBadge {
	objective := "Port the composer's activity strip and keep the benchmarks within budget"
	return goalStatus("goal.running", kit.Info, composer.Text{Label: "12m"}, composer.Text{Label: objective}, objective,
		"3 / 25", "12m", "96.4k", "pause", "end")
}

// Overview is c's session overview: the last turn's subagents, task progress
// and cited pages, and the transcript's files as its outputs.
func Overview(c *Conversation) *composer.Overview {
	o := &composer.Overview{Session: len(c.Turns) > 0}
	if !o.Session {
		return o
	}
	last := c.Turns[len(c.Turns)-1]
	o.Active = c.Running()
	seen := map[string]bool{}
	cite := func(list []conversation.Citation) {
		for _, source := range list {
			if !seen[source.URI] {
				seen[source.URI] = true
				o.Citations = append(o.Citations, source)
			}
		}
	}
	for _, b := range last.Reply.Blocks {
		switch b.Kind {
		case conversation.ProgressBlock:
			o.Steps = b.Steps
		case conversation.ChildrenBlock:
			for _, k := range b.Children {
				if k.Status == conversation.ChildRunning {
					o.Children = append(o.Children, composer.OverviewChild{ID: k.ID, Name: k.Title, Status: "chat_running", Running: true, Avatar: k.Avatar})
				}
			}
		}
		cite(b.Sources)
	}
	cite(last.Reply.Citations)
	for _, t := range c.Turns {
		for _, f := range t.Files {
			o.Sources = append(o.Sources, composer.OverviewAsset{ID: f.ID, Name: f.Path, Label: f.Name(), Icon: conversation.FileIcon(f)})
		}
		for _, b := range t.Reply.Blocks {
			for _, f := range b.Files {
				o.Output = append(o.Output, composer.OverviewAsset{ID: f.ID, Name: f.Name(), Icon: conversation.FileIcon(f)})
			}
		}
	}
	return o
}
