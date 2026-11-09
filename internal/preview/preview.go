// Package preview is PREVIEW DATA: an in-memory, deterministic dataset that
// stands in for Sailry's backend in this prototype. It never calls models,
// tools or the network, and never reads or writes user data. Views that show
// it say so with the "Preview" indicator.
package preview

import (
	"fmt"
	"strings"
	"time"
)

// Status is a conversation's state in the sidebar.
type Status int

const (
	Idle Status = iota
	Running
	Unread
	Waiting
)

// Role says who wrote a message.
type Role int

const (
	User Role = iota
	Assistant
)

// ToolStatus is the state of a tool call.
type ToolStatus int

const (
	ToolRunning ToolStatus = iota
	ToolDone
	ToolFailed
)

type Tool struct {
	Name    string // e.g. "Read", "Bash"
	Summary string // its arguments, one line
	Status  ToolStatus
}

type Message struct {
	ID   int
	Role Role
	Text string // Markdown for the assistant
	// Reasoning is the assistant's thinking, shown collapsed; Seconds how
	// long it took.
	Reasoning string
	Seconds   int
	Tools     []Tool
}

type Conversation struct {
	ID       string
	Title    string
	Status   Status
	Pinned   bool
	Updated  time.Time
	Messages []Message
}

type Project struct {
	ID            string
	Name          string
	Path          string
	Branch        string
	Conversations []*Conversation
}

// Data is the whole dataset.
type Data struct {
	Projects   []*Project
	Unassigned []*Conversation // chats without a project
	Models     []Model
	Providers  []Provider
	Plugins    []Plugin
}

type Model struct {
	ID, Name, Provider string
	Reasoning          bool
}

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

// Big is the ID of the conversation with 2000 messages.
const Big = "sailry-go/virtual-list"

// Load builds the dataset.
func Load() *Data {
	d := &Data{}
	n := 0
	conv := func(id, title string, st Status, ago time.Duration, msgs int) *Conversation {
		n++
		c := &Conversation{ID: id, Title: title, Status: st, Updated: Now.Add(-ago)}
		c.Messages = Messages(msgs, n)
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
	d.Projects[0].Conversations[2].Pinned = true
	d.Projects[1].Conversations[0].Pinned = true
	d.Unassigned = []*Conversation{
		conv("chat/regex", "Regex for semantic versions", Idle, 15*time.Minute, 4),
		conv("chat/translate", "Translate release notes", Unread, 2*time.Hour, 4),
		conv("chat/recipe", "Weeknight dinner ideas", Idle, 20*time.Hour, 4),
		conv("chat/sql", "Window functions explained", Idle, 3*24*time.Hour, 6),
		conv("chat/haiku", "Haiku about compilers", Idle, 7*24*time.Hour, 2),
		conv("chat/shell", "zsh completion errors", Idle, 11*24*time.Hour, 4),
	}
	d.Models = []Model{
		{"claude-opus-5-5", "Claude Opus 5.5", "Anthropic", true},
		{"claude-sonnet-5-5", "Claude Sonnet 5.5", "Anthropic", true},
		{"claude-haiku-5-5", "Claude Haiku 5.5", "Anthropic", false},
		{"gpt-5.2", "GPT-5.2", "OpenAI", true},
		{"gemini-3-pro", "Gemini 3 Pro", "Google", true},
		{"qwen3-coder", "Qwen3 Coder", "Ollama", false},
	}
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

var asks = []string{
	"Can you read `internal/conversation/list.go` and explain how rows are measured?",
	"Add a test that scrolls to the middle of the transcript.",
	"Why does the composer lose focus after sending?",
	"Rename `rowHeight` to `estimate` everywhere.",
	"Run the benchmarks again with GOMAXPROCS=1.",
}

var replies = []string{
	"The list measures each row the first time it shows and **estimates** the rest from the average, so the scroll bar settles as you move.\n\n- Rows keep their place by key, not by offset.\n- Heights are cached per width.\n\nThe relevant part:\n\n```go\nfunc (l *list) estimate(i int) float32 {\n\tif h, ok := l.measured[i]; ok {\n\t\treturn h\n\t}\n\treturn l.average\n}\n```",
	"Done. The new test scrolls by `ScrollTo(1000, ui.Center)` and checks that message **#1000** is on screen.\n\n| Step | Result |\n|---|---|\n| Build | ok |\n| `go test ./...` | ok |",
	"Focus moves to the send button because it is clicked with the pointer. Keeping the focus in the input is one line:\n\n```go\napp.input.Focus()\n```\n\nI applied it after `send()`.",
	"Renamed in 4 files. No public API changed.",
	"Results on this machine:\n\n```text\nBenchmarkScroll-1   2000   412 µs/op   0 B/op\n```\n\nThat is well under a 16.7 ms frame.",
}

var tools = [][]Tool{
	{{"Read", "internal/conversation/list.go", ToolDone}, {"Grep", "\"estimate\" in internal/", ToolDone}},
	{{"Edit", "internal/conversation/view_test.go", ToolDone}, {"Bash", "go test ./internal/conversation", ToolDone}},
	{{"Read", "internal/composer/composer.go", ToolDone}},
	{{"Bash", "gofmt -l .", ToolFailed}, {"Edit", "internal/conversation/list.go", ToolDone}},
	{{"Bash", "go test -bench . -run ^$", ToolRunning}},
}

// Messages makes n messages alternating between the user and the
// assistant; seed varies them between conversations.
func Messages(n, seed int) []Message {
	out := make([]Message, n)
	for i := range out {
		k := (i/2 + seed) % len(asks)
		m := Message{ID: i + 1}
		if i%2 == 0 {
			m.Role, m.Text = User, asks[k]
		} else {
			m.Role, m.Text = Assistant, replies[k]
			if i%4 == 1 {
				m.Reasoning = reasoning(k)
				m.Seconds = 3 + (i*7)%40
			}
			if i%3 != 0 {
				m.Tools = tools[k]
				if i < n-1 && m.Tools[len(m.Tools)-1].Status == ToolRunning {
					// Only the newest reply still runs a tool.
					m.Tools = append([]Tool(nil), m.Tools...)
					m.Tools[len(m.Tools)-1].Status = ToolDone
				}
			}
		}
		out[i] = m
	}
	return out
}

func reasoning(k int) string {
	return strings.TrimSpace(fmt.Sprintf(`The user asks about step %d. I should check the file first, then answer briefly with the relevant code.
The list builds only the rows in view, so the cost depends on what shows, not on how many messages there are.`, k+1))
}
