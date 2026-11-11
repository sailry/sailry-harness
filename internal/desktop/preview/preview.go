// Package preview is PREVIEW DATA: an in-memory, deterministic dataset that
// stands in for Sailry's backend in this prototype. It never calls models,
// tools or the network, and never reads or writes user data. The preview
// window says so with the "Preview" indicator; the demo (demo.go), shown by
// Demo mode over a live window, has the same bounds and no indicator.
package preview

import (
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type Status = viewmodel.Status
type Conversation = viewmodel.Conversation
type Project = viewmodel.Project
type Data = viewmodel.Data
type Model = viewmodel.Model
type Provider = viewmodel.Provider
type Plugin = viewmodel.Plugin

const (
	Idle    = viewmodel.Idle
	Running = viewmodel.Running
	Unread  = viewmodel.Unread
	Waiting = viewmodel.Waiting
)

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
	simulation(big).queued = 2
	big.Commands, big.Badges = background(), []composer.StatusBadge{goal()}
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
	// Long histories load as the live view's do: the last page shows, the
	// older ones load a page at a time.
	paged(d.Find(Big))
	paged(d.Unassigned[len(d.Unassigned)-3])
	off := d.Unassigned[len(d.Unassigned)-2]
	off.Session.Connection = conversation.Reconnecting
	off.Session.Diagnostic = &conversation.Diagnostic{Title: "Model request rejected", Fields: [][2]string{
		{"provider", "anthropic"}, {"status", "400"}, {"type", "invalid_request_error"}, {"request", "req_01J9…"},
	}, Detail: `{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens: 64000 > 32000"}}`}
	d.Unassigned[len(d.Unassigned)-1].Session.Loading = true
	d.Models = models()
	d.Providers = []Provider{
		{ID: "anthropic", Name: "Anthropic", Description: "Claude models with OAuth or an API key", Badge: "provider_auth_oauth", Enabled: true},
		{ID: "openai", Name: "OpenAI", Description: "GPT models through the Responses API", Badge: "provider_auth_key", Enabled: true},
		{ID: "google", Name: "Google", Description: "Gemini models through AI Studio", Badge: "provider_status_missing_key", Enabled: false},
		{ID: "openrouter", Name: "OpenRouter", Description: "Hundreds of models behind one key", Badge: "provider_auth_key", Enabled: false},
		{ID: "ollama", Name: "Ollama", Description: "Local models on this computer", Badge: "provider_auth_plugin", Enabled: true},
		{ID: "deepseek", Name: "DeepSeek", Description: "DeepSeek chat and reasoning models", Badge: "provider_status_signed_out", Enabled: false},
	}
	d.Plugins = []Plugin{
		{ID: "git", Name: "Git", Description: "Branches, diffs and commits in conversations", Version: "0.3.2", Official: true, Enabled: true},
		{ID: "terminal", Name: "Terminal", Description: "Ghostty terminals beside the conversation", Version: "0.2.0", Official: true, Enabled: true},
		{ID: "browser", Name: "Browser", Description: "Pages the agent reads and annotates", Version: "0.1.4", Official: true, Enabled: false},
		{ID: "knowledge", Name: "Knowledge", Description: "Local documents searched by meaning", Version: "0.1.1", Official: true, Enabled: true},
		{ID: "figma", Name: "Figma", Description: "Frames and comments from design files", Version: "0.1.0", Official: false, Enabled: false},
		{ID: "linear", Name: "Linear", Description: "Issues linked to conversations", Version: "0.2.3", Official: false, Enabled: true},
	}
	return d
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
