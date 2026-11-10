//go:build integration

package live

import (
	"context"
	"fmt"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The B13 system acceptance drives the real Settings pages over a real
// temporary Node and Client. This file holds what its tests share; each
// feature has its own system_*_integration_test.go.

// systemUIWait bounds every wait of the B13 system tests.
const systemUIWait = 15 * time.Second

// systemDesk is a temporary Node with a controller beside the window's.
type systemDesk struct {
	t       *testing.T
	ctx     context.Context
	profile string
	owner   *node.Node
	setup   *client.Client
}

func newSystemDesk(t *testing.T, ctx context.Context, profile string, options node.Options) *systemDesk {
	t.Helper()
	owner, err := node.StartWithOptions(ctx, profile, options)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = owner.Stop(context.Background()) })
	setup := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := setup.Start(ctx); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = setup.Close() })
	return &systemDesk{t: t, ctx: ctx, profile: profile, owner: owner, setup: setup}
}

func (desk *systemDesk) do(command protocol.Command) protocol.Output {
	desk.t.Helper()
	output, err := desk.setup.Do(desk.ctx, command)
	if err != nil {
		desk.t.Fatalf("%s: %v", command.Kind, err)
	}
	return output
}

// project registers a local folder as a project.
func (desk *systemDesk) project(name string) protocol.Project {
	desk.t.Helper()
	folder, err := filepath.EvalSymlinks(desk.t.TempDir())
	if err != nil {
		desk.t.Fatal(err)
	}
	output := desk.do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: name, Path: folder, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}})
	project, valid := detail[protocol.Project](output.Data)
	if !valid {
		desk.t.Fatal("registered project was not returned")
	}
	return project
}

// provider saves a provider no request is ever sent to, unless endpoint
// names a fixture server.
func (desk *systemDesk) provider(endpoint string) (protocol.ConversationProvider, protocol.ConversationModel) {
	desk.t.Helper()
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "System fixture", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, endpoint
	provider.Enabled, provider.DefaultModel = true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output, model.Tools = "fixture", 200000, 8192, true
	model.Reasoning, model.DefaultEffort = true, protocol.Effort{Kind: "none"}
	model.Efforts = []protocol.Effort{model.DefaultEffort}
	provider.Models = []protocol.ConversationModel{model}
	secret := protocol.NewSecret("fixture-provider-key")
	desk.do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	return provider, model
}

// session creates a session of the project and names it.
func (desk *systemDesk) session(project protocol.Project, provider protocol.ConversationProvider, model protocol.ConversationModel, title string) protocol.Session {
	desk.t.Helper()
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	output := desk.do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}})
	session, valid := detail[protocol.Session](output.Data)
	if !valid {
		desk.t.Fatal("created session was not returned")
	}
	if title != "" {
		output = desk.do(protocol.Command{Kind: "rename_session", Data: protocol.CommandRenameSessionPayload{Session: session.ID, ExpectedRevision: session.Revision, Title: title}})
		if renamed, valid := detail[protocol.Session](output.Data); valid {
			session = renamed
		}
	}
	return session
}

// current is the Node's session now.
func (desk *systemDesk) current(id protocol.SessionID) (protocol.Session, bool) {
	for _, session := range desk.setup.Snapshot().Sessions {
		if session.ID == id {
			return session, true
		}
	}
	return protocol.Session{}, false
}

// labelled is the single visible input with an accessible name.
func labelled(w *terminalUI, label string) ui.AccessInfo {
	w.t.Helper()
	return w.one(ui.RoleTextField, label)
}

// fill types text over the whole value of the input named label. Password
// inputs do not expose their value, so only plain ones are read back.
func fill(w *terminalUI, label, text string, secret bool) {
	w.t.Helper()
	settle(w, fmt.Sprintf("the enabled input %q", label), systemUIWait, func() bool {
		found := w.named(ui.RoleTextField, label)
		return len(found) == 1 && !found[0].Disabled
	})
	w.clickOn(reveal(w, ui.RoleTextField, label))
	if !labelled(w, label).Focused {
		w.t.Fatalf("the input %q did not take the focus; elements: %s", label, w.elements())
	}
	w.tester.Key(ui.Cmd, ui.KeyA)
	if text == "" {
		w.tester.Key(0, ui.KeyBackspace)
	} else {
		w.tester.Type(text)
	}
	w.frame()
	if value := labelled(w, label).Value; !secret && value != text {
		w.t.Fatalf("input %q holds %q after typing %q", label, value, text)
	}
}

// toast waits for a toast and then lets the toasts run out, so the next
// one with the same text is a new one.
func toast(w *terminalUI, text string) {
	w.t.Helper()
	seen := lingering[w]
	if seen == nil {
		seen = map[string]int{}
		lingering[w] = seen
	}
	settle(w, fmt.Sprintf("a new toast %q (%d linger)", text, seen[text]), systemUIWait, func() bool { return shown(w, text) > seen[text] })
	seen[text] = shown(w, text)
	// Done toasts run out on the frame clock. Failure and conflict toasts
	// stay until closed, and one under an open dialog cannot be reached, so
	// whatever still shows is counted and the next wait asks for one more.
	for range 48 {
		w.tester.Advance(250 * time.Millisecond)
		w.tester.Frame()
	}
	for range 6 {
		closed := false
		// A toast's Close is its 20-point icon button; a dialog's is wider.
		for _, candidate := range w.buttons("Close") {
			if candidate.Bounds.W <= 24 {
				w.clickOn(candidate)
				closed = true
				break
			}
		}
		if !closed {
			break
		}
		for range 20 {
			w.frame()
		}
	}
	for known := range seen {
		seen[known] = shown(w, known)
	}
}

// lingering counts, by window, the toasts that still show after a wait.
var lingering = map[*terminalUI]map[string]int{}

// noToast fails when a new toast with text shows.
func noToast(w *terminalUI, text string) {
	w.t.Helper()
	for range 5 {
		w.frame()
	}
	if shown(w, text) > lingering[w][text] {
		w.t.Fatalf("the toast %q shows; visible text: %q", text, w.tester.Texts())
	}
}

// beside clicks the enabled button named name on the row of the single
// button named anchor, as a dialog's action beside its Cancel.
func footerAction(w *terminalUI, anchor, name string) {
	w.t.Helper()
	var target ui.AccessInfo
	settle(w, fmt.Sprintf("the %q button beside %q", name, anchor), systemUIWait, func() bool {
		anchors := w.buttons(anchor)
		if len(anchors) != 1 {
			return false
		}
		_, y := center(anchors[0].Bounds)
		for _, candidate := range w.buttons(name) {
			if _, cy := center(candidate.Bounds); cy > y-6 && cy < y+6 && candidate.Bounds.X > anchors[0].Bounds.X && !candidate.Disabled {
				target = candidate
				return true
			}
		}
		return false
	})
	w.clickOn(target)
}

// visible reports whether any shown text contains part.
func visible(w *terminalUI, part string) bool {
	for _, text := range w.tester.Texts() {
		if strings.Contains(text, part) {
			return true
		}
	}
	return false
}

// leaked reports whether a secret shows as text or as any element's name,
// value or placeholder.
func leaked(w *terminalUI, secret string) bool {
	if visible(w, secret) {
		return true
	}
	for _, entry := range w.tester.AccessAll() {
		if strings.Contains(entry.Name, secret) || strings.Contains(entry.Value, secret) || strings.Contains(entry.Placeholder, secret) {
			return true
		}
	}
	return false
}

// reveal scrolls a dialog's body until the single element of a role named
// name lies above the dialog's footer, and returns it there. Without a
// Cancel button no dialog with a footer is open and nothing scrolls.
func reveal(w *terminalUI, role ui.Role, name string) ui.AccessInfo {
	w.t.Helper()
	direction := float32(1)
	for range 40 {
		target := w.one(role, name)
		cancels := w.buttons("Cancel")
		if len(cancels) != 1 || target.Bounds.Y+target.Bounds.H <= cancels[0].Bounds.Y-8 {
			return target
		}
		x, _ := center(target.Bounds)
		w.tester.Scroll(x, cancels[0].Bounds.Y-80, 0, 60*direction)
		w.frame()
		if moved := w.one(role, name); moved.Bounds.Y > target.Bounds.Y {
			direction = -direction
		}
	}
	w.t.Fatalf("the %q of the dialog cannot be scrolled into view; elements: %s", name, w.elements())
	return ui.AccessInfo{}
}

// pressIn clicks the single enabled button named name of the open dialog,
// scrolling to it first.
func pressIn(w *terminalUI, name string) {
	w.t.Helper()
	settle(w, fmt.Sprintf("one enabled button %q", name), systemUIWait, func() bool {
		found := w.named(ui.RoleButton, name)
		return len(found) == 1 && !found[0].Disabled
	})
	w.clickOn(reveal(w, ui.RoleButton, name))
}

// closeDialog clicks the footer's Close of the open dialog: the widest of
// the buttons named Close, beside the dialog's corner icon and the toasts'.
func closeDialog(w *terminalUI) {
	w.t.Helper()
	closes := w.buttons("Close")
	if len(closes) == 0 {
		w.t.Fatalf("no Close button; elements: %s", w.elements())
	}
	widest := closes[0]
	for _, candidate := range closes {
		if candidate.Bounds.W > widest.Bounds.W {
			widest = candidate
		}
	}
	if widest.Bounds.W <= 32 {
		w.t.Fatalf("no footer Close button; elements: %s", w.elements())
	}
	w.clickOn(widest)
}
