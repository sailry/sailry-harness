// Package preview is PREVIEW DATA: an in-memory, deterministic dataset that
// stands in for Sailry's backend in this prototype. It never calls models,
// tools or the network, and never reads or writes user data. Views that show
// it say so with the "Preview" indicator.
package preview

import (
	"time"

	"github.com/sailry/sailry-go/internal/composer"
	"github.com/sailry/sailry-go/internal/conversation"
)

// Status is a conversation's state in the sidebar.
type Status int

const (
	Idle Status = iota
	Running
	Unread
	Waiting
)

type Conversation struct {
	ID     string
	Title  string
	Status Status
	Pinned bool
	// PinOrder is where a pin goes among the pinned (preferences/pins.rs):
	// nil for a pin made before pins had an order.
	PinOrder *int
	Archived bool
	Updated  time.Time
	// Created is the order the conversation was made in, newest highest.
	Created  int
	Worktree string // the branch of its worktree; "" is the project's main one
	Turns    []conversation.Turn
	// Waiting is what the running reply waits on the user for.
	Waiting composer.Interactions
	// Queue is the messages sent while the reply runs.
	Queue  []composer.Queued
	queued int // the queue's messages made so far
	// Session is the conversation around its turns: older pages, the
	// connection, a diagnostic and usage.
	Session conversation.Session

	stream *stream             // the reply the preview plays after a send
	older  []conversation.Turn // the turns before the first shown
	paging time.Time           // when the page asked for started loading
}

type Project struct {
	ID            string
	Name          string
	Path          string
	Branch        string
	Host          string // the Host's ID
	Worktrees     []Worktree
	Trusted       bool
	Conversations []*Conversation
}

// Data is the whole dataset.
type Data struct {
	Projects   []*Project
	Unassigned []*Conversation // chats without a project
	Models     []Model
	Providers  []Provider
	Plugins    []Plugin
	*Workspace
}

type Model = composer.Model

type Provider struct {
	ID, Name, Description string
	Badge                 string // i18n key or ""
	Enabled               bool
}

type Plugin struct {
	ID, Name, Description, Version string
	Official, Enabled              bool
}

// Now is the fixed clock of the dataset, so ages render the same in tests.
var Now = time.Date(2026, 11, 9, 12, 0, 0, 0, time.UTC)

// Big is the ID of the conversation with 1000 turns.
const Big = "sailry-go/virtual-list"

// Load builds the dataset.
func Load() *Data {
	d := &Data{}
	n := 0
	conv := func(id, title string, st Status, ago time.Duration, msgs int) *Conversation {
		n++
		c := &Conversation{ID: id, Title: title, Status: st, Updated: Now.Add(-ago), Created: 1000 - int(ago/time.Minute)}
		c.Turns = Turns(id, msgs/2, n)
		if st == Running {
			c.Turns[len(c.Turns)-1] = live(c.Turns[len(c.Turns)-1].ID)
			c.Waiting = waiting(c.Turns[len(c.Turns)-1].ID)
		}
		stamp(c.Turns)
		c.Session.Usage = usage(len(c.Turns))
		return c
	}
	d.Projects = []*Project{
		{ID: "sailry-go", Name: "sailry-go", Path: "~/Web/sailry-go", Branch: "main", Conversations: []*Conversation{
			conv(Big, "Virtualized transcript stress test", Running, 2*time.Minute, 2000),
			conv("sailry-go/theme", "Port the neutral theme tokens", Unread, 25*time.Minute, 14),
			conv("sailry-go/i18n", "Copy locale keys for the shell", Idle, 3*time.Hour, 8),
			conv("sailry-go/split", "Resizable sidebar split", Idle, 26*time.Hour, 6),
			conv("sailry-go/tests", "Tester coverage for the composer", Waiting, 2*24*time.Hour, 10),
			conv("sailry-go/settings", "Settings providers list", Idle, 3*24*time.Hour, 4),
			conv("sailry-go/icons", "Icon tiles for plugins", Idle, 5*24*time.Hour, 4),
		}},
		{ID: "sailry", Name: "sailry-public", Path: "~/Web/sailry-public", Branch: "main", Conversations: []*Conversation{
			conv("sailry/release", "Windows release lint fixes", Idle, 40*time.Minute, 12),
			conv("sailry/mcp", "Stabilize MCP warm-up tests", Unread, 5*time.Hour, 16),
			conv("sailry/assets", "Mutable release assets", Idle, 30*time.Hour, 6),
			conv("sailry/compile", "Reuse verified compilation", Idle, 2*24*time.Hour, 8),
			conv("sailry/imports", "Conditional imports on Windows", Idle, 4*24*time.Hour, 6),
			conv("sailry/ci", "CI macOS capacity notes", Idle, 6*24*time.Hour, 4),
			conv("sailry/theme", "Theme package contrast check", Idle, 8*24*time.Hour, 6),
			conv("sailry/connectors", "Connectors menu deviations", Idle, 9*24*time.Hour, 4),
		}},
		{ID: "notes", Name: "field-notes", Path: "~/Documents/field-notes", Branch: "draft", Conversations: []*Conversation{
			conv("notes/outline", "Outline for the trip report", Idle, 7*time.Hour, 6),
			conv("notes/photos", "Rename photos by date", Idle, 3*24*time.Hour, 4),
			conv("notes/budget", "Budget table cleanup", Idle, 12*24*time.Hour, 4),
		}},
		{ID: "api", Name: "billing-api", Path: "~/Work/billing-api", Branch: "feature/invoices", Conversations: []*Conversation{
			conv("api/invoices", "Invoice PDF rendering", Running, 1*time.Minute, 10),
			conv("api/webhooks", "Webhook retries with backoff", Idle, 9*time.Hour, 8),
			conv("api/migrations", "Migration for tax rates", Idle, 2*24*time.Hour, 6),
			conv("api/tests", "Flaky integration test", Waiting, 4*24*time.Hour, 6),
			conv("api/docs", "OpenAPI descriptions", Idle, 10*24*time.Hour, 4),
		}},
	}
	order := func(n int) *int { return &n }
	d.Projects[0].Conversations[2].Pinned, d.Projects[0].Conversations[2].PinOrder = true, order(0)
	d.Projects[1].Conversations[0].Pinned, d.Projects[1].Conversations[0].PinOrder = true, order(1)
	d.Projects[3].Conversations[1].Pinned = true // a pin from before pins had an order
	d.Projects[0].Conversations[3].Worktree = "split-view"
	d.Projects[0].Conversations[6].Archived = true
	// The running conversation has messages waiting behind its reply.
	big := d.Projects[0].Conversations[0]
	big.Queue = []composer.Queued{
		{ID: Big + "/queued-1", Text: "Then run the benchmarks again with 500 turns"},
		{ID: Big + "/queued-2", Text: "Add the numbers to the report", Attachments: []*composer.Attachment{
			{ID: Big + "/queued-2/report", Name: "benchmarks.md", Mime: "text/markdown", Size: 2458, Status: composer.Ready, Progress: 1},
		}},
	}
	big.queued = 2
	for _, p := range d.Projects {
		p.Host, p.Trusted = "local", true
		p.Worktrees = []Worktree{{Branch: p.Branch, Path: p.Path, Main: true}}
	}
	d.Projects[0].Worktrees = append(d.Projects[0].Worktrees, Worktree{Branch: "split-view", Path: "~/Web/sailry-go-split-view"})
	d.Projects[3].Host, d.Projects[3].Trusted = "remote", false
	d.Workspace = workspace()
	d.Unassigned = []*Conversation{
		conv("chat/regex", "Regex for semantic versions", Idle, 15*time.Minute, 4),
		conv("chat/translate", "Translate release notes", Unread, 2*time.Hour, 4),
		conv("chat/recipe", "Weeknight dinner ideas", Idle, 20*time.Hour, 4),
		conv("chat/sql", "Window functions explained", Idle, 3*24*time.Hour, 6),
		conv("chat/haiku", "Haiku about compilers", Idle, 7*24*time.Hour, 2),
		conv("chat/shell", "zsh completion errors", Idle, 11*24*time.Hour, 4),
		conv("chat/paging", "Long history, paged", Idle, 13*24*time.Hour, 64),
		conv("chat/offline", "Connection recovering", Idle, 14*24*time.Hour, 6),
		conv("chat/loading", "History loading", Idle, 15*24*time.Hour, 4),
	}
	// Older pages: the last 8 turns show, the rest load 8 at a time.
	pg := d.Unassigned[len(d.Unassigned)-3]
	pg.older, pg.Turns = pg.Turns[:len(pg.Turns)-8], pg.Turns[len(pg.Turns)-8:]
	pg.Session.Older = len(pg.older)
	off := d.Unassigned[len(d.Unassigned)-2]
	off.Session.Connection = conversation.Reconnecting
	off.Session.Diagnostic = &conversation.Diagnostic{Title: "Model request rejected", Fields: [][2]string{
		{"provider", "anthropic"}, {"status", "400"}, {"type", "invalid_request_error"}, {"request", "req_01J9…"},
	}, Detail: `{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens: 64000 > 32000"}}`}
	d.Unassigned[len(d.Unassigned)-1].Session.Loading = true
	d.Models = models()
	d.Providers = []Provider{
		{"anthropic", "Anthropic", "Claude models with OAuth or an API key", "provider_auth_oauth", true},
		{"openai", "OpenAI", "GPT models through the Responses API", "provider_auth_key", true},
		{"google", "Google", "Gemini models through AI Studio", "provider_status_missing_key", false},
		{"openrouter", "OpenRouter", "Hundreds of models behind one key", "provider_auth_key", false},
		{"ollama", "Ollama", "Local models on this computer", "provider_auth_plugin", true},
		{"deepseek", "DeepSeek", "DeepSeek chat and reasoning models", "provider_status_signed_out", false},
	}
	d.Plugins = []Plugin{
		{"git", "Git", "Branches, diffs and commits in conversations", "0.3.2", true, true},
		{"terminal", "Terminal", "Ghostty terminals beside the conversation", "0.2.0", true, true},
		{"browser", "Browser", "Pages the agent reads and annotates", "0.1.4", true, false},
		{"knowledge", "Knowledge", "Local documents searched by meaning", "0.1.1", true, true},
		{"figma", "Figma", "Frames and comments from design files", "0.1.0", false, false},
		{"linear", "Linear", "Issues linked to conversations", "0.2.3", false, true},
	}
	return d
}

// Find returns the conversation with id.
func (d *Data) Find(id string) *Conversation {
	for _, p := range d.Projects {
		for _, c := range p.Conversations {
			if c.ID == id {
				return c
			}
		}
	}
	for _, c := range d.Unassigned {
		if c.ID == id {
			return c
		}
	}
	return nil
}

// ProjectOf returns the project holding the conversation, nil when none.
func (d *Data) ProjectOf(id string) *Project {
	for _, p := range d.Projects {
		for _, c := range p.Conversations {
			if c.ID == id {
				return p
			}
		}
	}
	return nil
}

// waiting is what a live reply waits on: two approvals, one a change set,
// and a question.
func waiting(id string) composer.Interactions {
	return composer.Interactions{
		Approvals: []composer.Approval{
			{ID: call(id, "write", "", "", "internal/theme/controls.go").ID, Title: "Write file", Context: "~/Web/sailry-go", Changes: []composer.Change{
				{Path: "internal/theme/controls.go", Kind: composer.Modified, Added: 24, Removed: 3},
				{Path: "internal/kit/loader.go", Kind: composer.Added, Added: 41},
			}, Remember: true},
			{ID: call(id, "command", "", "", "go vet ./...").ID, Title: "Command", Context: "~/Web/sailry-go", Description: "go vet ./...",
				Code: true, Remember: true, Prefix: "go vet"},
		},
		Question: &composer.Ask{ID: id + "/ask", Prompt: "Keep the 2x3 loader or use Kit's spinner?",
			Options: []string{"Keep the loader", "Use the spinner"}, Other: true},
	}
}
