package live

import (
	"context"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// railPagesCountingTransport counts the directory listings and the file
// subscriptions a desktop sends to its Node.
type railPagesCountingTransport struct {
	link.Transport
	mu            sync.Mutex
	listings      int
	subscriptions int
}

func (t *railPagesCountingTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind == "list_directory" {
		t.mu.Lock()
		t.listings++
		t.mu.Unlock()
	}
	return t.Transport.Dispatch(ctx, request)
}

func (t *railPagesCountingTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "files" || topic.Kind == "repository" {
		t.mu.Lock()
		t.subscriptions++
		t.mu.Unlock()
	}
	return t.Transport.Subscribe(ctx, topic)
}

func (t *railPagesCountingTransport) counts() (int, int) {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.listings, t.subscriptions
}

// railPagesSession shows the rail's Files page of a session without a
// project against a real Node: its empty folder is listed once, and frames
// in which nothing changes neither list it again nor subscribe again. It
// runs as a step of TestPanelProjection.
func railPagesSession(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Minute)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.StartWithOptions(ctx, profile, node.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	transport := &railPagesCountingTransport{Transport: link.NewLocal(owner.ID(), owner.ID(), owner)}
	controller := client.New(transport)
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(ctx, controller, preferences.ForStartup(profile))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	tester := ui.NewTesterWith(r.App.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	text := func(key string) string { return i18n.In("en", key) }
	app := r.App
	settle := func() {
		t.Helper()
		for range 6 {
			r.workers.Wait()
			tester.Advance(200 * time.Millisecond)
			tester.Frame()
		}
	}
	wait := func(what string, done func() bool) {
		t.Helper()
		for end := time.Now().Add(20 * time.Second); time.Now().Before(end); time.Sleep(10 * time.Millisecond) {
			settle()
			if done() {
				return
			}
		}
		t.Fatalf("timed out waiting for %s; visible text: %q", what, tester.Texts())
	}
	settle()
	if tester.HasText(text("close")) {
		if err := tester.Click(text("close")); err != nil {
			t.Fatal(err)
		}
		settle()
	}
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Rail fixture", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "http://127.0.0.1:9/v1"
	provider.Enabled, provider.DefaultModel = true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output = "fixture", 200000, 8192
	model.Reasoning, model.DefaultEffort = true, protocol.Effort{Kind: "none"}
	model.Efforts = []protocol.Effort{model.DefaultEffort}
	provider.Models = []protocol.ConversationModel{model}
	secret := protocol.NewSecret("fixture-key")
	do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	session, valid := detail[protocol.Session](do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}).Data)
	if !valid || session.Project != nil {
		t.Fatalf("the created session: %+v", session)
	}
	wait("the session in the sidebar", func() bool {
		r.OpenSession(session.ID.String())
		return app.Sidebar.Selected == session.ID.String() && r.workspace != nil && r.workspace.selected == session.Worktree
	})
	for _, info := range tester.AccessAll() {
		if info.Role == ui.RoleButton && info.Name == text("files") && info.Bounds.X+info.Bounds.W <= shell.RailWidth {
			tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
			break
		}
	}
	wait("the session's empty folder on the Files page", func() bool {
		return app.Page == shell.FilesPage && tester.HasText(text("files_directory_empty"))
	})
	// Changes the Node made while the session opened are debounced for
	// 200 ms; let them arrive before the quiet frames are counted.
	for range 5 {
		time.Sleep(100 * time.Millisecond)
		settle()
	}
	listings, subscriptions := transport.counts()
	for range 20 {
		time.Sleep(50 * time.Millisecond)
		settle()
	}
	if after, again := transport.counts(); after != listings || again != subscriptions {
		t.Fatalf("an unchanged Files page of a session read on: list_directory %d -> %d, file subscriptions %d -> %d",
			listings, after, subscriptions, again)
	}
	if !tester.HasText(text("files_directory_empty")) {
		t.Fatalf("the Files page lost the empty folder: %q", tester.Texts())
	}
}
