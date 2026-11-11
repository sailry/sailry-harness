package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// hookedScript is a scripted model that lets the test act between two of the
// turn's model requests.
type hookedScript struct {
	*mediaScript
	before func(step int)
}

func (script *hookedScript) Stream(ctx context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	step := len(script.offered) + 1
	script.mu.Unlock()
	if script.before != nil {
		script.before(step)
	}
	return script.mediaScript.Stream(ctx, call)
}

// TestBrowserToolBinding proves which turns get the browser package's tools
// and that both browsers follow their conversation and their package as they
// are now. No browser is started.
func TestBrowserToolBinding(t *testing.T) {
	tabs := operations.Alias("browser", "browser_tabs")
	discover := [][3]string{{"discover", "tool_search", `{"query":"select:` + tabs + `"}`}}
	absent := &hookedScript{mediaScript: &mediaScript{results: map[string]string{}, steps: [][][3]string{discover}}}
	present := &hookedScript{mediaScript: &mediaScript{results: map[string]string{}, steps: [][][3]string{
		discover, {{"first", tabs, `{}`}}, {{"second", tabs, `{}`}},
	}}}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	directory := filepath.Join(t.TempDir(), "profile")
	options := Options{
		Environment: &environment, Model: func(_ context.Context, binding model.Binding) (fantasy.LanguageModel, error) {
			switch binding.Config.Model {
			case "absent":
				return absent, nil
			case "present":
				return present, nil
			}
			return nil, errors.New("unexpected model " + binding.Config.Model)
		},
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("browser binding must not launch a process")
		}},
	}
	node, err := StartWithOptions(t.Context(), directory, options)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { node.Stop(context.Background()) }()
	secret := protocol.NewSecret("browser-key")
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Browser fixture", true
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://127.0.0.1:1/v1", "absent"
	for _, id := range []string{"absent", "present"} {
		entry := protocol.DefaultConversationModel()
		entry.ID, entry.Context, entry.Output, entry.Tools, entry.CustomCapabilities = id, 32000, 8000, true, true
		provider.Models = append(provider.Models, entry)
	}
	commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project := commandFixture(t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Browser binding", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	create := func(name string) protocol.Session {
		t.Helper()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, name, protocol.WorkModeCode, protocol.PermissionFull
		return commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data.(protocol.Session)
	}
	changes, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer changes.Close()
	run := func(t *testing.T, name string) {
		t.Helper()
		session := create(name)
		commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
			Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: "List the tabs"},
		}})
		for completed := false; !completed; {
			var history protocol.ConversationHistory
			if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
				var err error
				history, err = store.ReadConversation(t.Context(), db, session.ID, nil, 100)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			for _, run := range history.Page.Runs {
				switch run.Status {
				case protocol.ConversationStatusCompleted:
					completed = true
				case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
					t.Fatalf("browser turn failed: %+v", run)
				}
			}
			if !completed {
				if _, err := changes.Next(t.Context()); err != nil {
					t.Fatal(err)
				}
			}
		}
	}
	offers := func(script *hookedScript) bool {
		script.mu.Lock()
		defer script.mu.Unlock()
		return slices.ContainsFunc(script.offered, func(names []string) bool { return slices.Contains(names, tabs) })
	}

	t.Run("a turn whose controller has no browser is offered no browser tools", func(t *testing.T) {
		run(t, "absent")
		if offers(absent) {
			absent.mu.Lock()
			defer absent.mu.Unlock()
			t.Fatalf("%s was offered without a browser: %q", tabs, absent.results["discover"])
		}
	})

	t.Run("a browser tool follows its package as it is now", func(t *testing.T) {
		// This controller has a browser from here on.
		calls, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "browser"})
		if err != nil {
			t.Fatal(err)
		}
		defer calls.Close()
		var served atomic.Int32
		go func() {
			for {
				update, err := calls.Next(t.Context())
				if err != nil {
					return
				}
				call := update.Data.(protocol.BrowserCall)
				served.Add(1)
				listed := json.RawMessage(`{"tabs":[]}`)
				_ = node.browsers.complete(node.ID(), call.ID, protocol.BrowserResult{Ok: &listed})
			}
		}()
		// The user disables the package after the turn's first browser call.
		present.before = func(step int) {
			if step != 3 {
				return
			}
			accepted, err := node.Dispatch(context.Background(), node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "browser"}}))
			if err != nil {
				t.Error(err)
				return
			}
			info := (<-accepted.Completion).Output.Data.(protocol.PluginInfo)
			accepted, err = node.Dispatch(context.Background(), node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "set_plugin_enabled",
				Data: protocol.CommandSetPluginEnabledPayload{Name: "browser", ExpectedRevision: info.Summary.Revision, Enabled: false}}))
			if err != nil {
				t.Error(err)
				return
			}
			if outcome := <-accepted.Completion; outcome.Fault != nil {
				t.Error(outcome.Fault)
			}
		}
		run(t, "present")
		present.mu.Lock()
		results := present.results
		present.mu.Unlock()
		if !offers(present) {
			t.Fatalf("%s was not offered to a controller with a browser: %q", tabs, results["discover"])
		}
		if !strings.Contains(results["first"], `"tabs"`) || strings.Contains(results["first"], "error") {
			t.Fatalf("browser call of an enabled package = %q", results["first"])
		}
		if !strings.Contains(results["second"], "plugin is disabled") || served.Load() != 1 {
			t.Fatalf("browser call of a disabled package = %q after %d calls reached the browser", results["second"], served.Load())
		}
	})

	t.Run("a managed browser profile goes with its conversation", func(t *testing.T) {
		profile := func(session protocol.SessionID) string {
			path := filepath.Join(directory, "browser", "sessions", session.String(), "profile")
			if err := os.MkdirAll(path, 0o700); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(path, "Cookies"), []byte("signed in"), 0o600); err != nil {
				t.Fatal(err)
			}
			return filepath.Dir(path)
		}
		gone := func(path string) bool {
			_, err := os.Stat(path)
			return errors.Is(err, os.ErrNotExist)
		}
		settled := func(what string, reached func() bool) {
			t.Helper()
			for deadline := time.Now().Add(10 * time.Second); !reached(); time.Sleep(5 * time.Millisecond) {
				if time.Now().After(deadline) {
					t.Fatal(what)
				}
			}
		}
		removed, archived, kept := create("absent"), create("absent"), create("absent")
		removedProfile, archivedProfile, keptProfile := profile(removed.ID), profile(archived.ID), profile(kept.ID)
		commandFixture(t, node, protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{Session: archived.ID, ExpectedRevision: archived.Revision, Archived: true}})
		commandFixture(t, node, protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: removed.ID, ExpectedRevision: removed.Revision}})
		settled("a removed conversation kept its browser profile", func() bool { return gone(removedProfile) })
		if gone(archivedProfile) || gone(keptProfile) {
			t.Fatalf("profiles of live conversations: archived gone %v, open gone %v", gone(archivedProfile), gone(keptProfile))
		}

		// A profile whose conversation went while no Node could delete it is
		// deleted when the next Node starts; the others stay.
		orphan := profile(protocol.NewSessionID())
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		node, err = StartWithOptions(t.Context(), directory, options)
		if err != nil {
			t.Fatal(err)
		}
		if !gone(orphan) || gone(archivedProfile) || gone(keptProfile) {
			t.Fatalf("after a restart: orphan gone %v, archived gone %v, open gone %v", gone(orphan), gone(archivedProfile), gone(keptProfile))
		}
	})
}
