//go:build integration && !windows

package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/externalbrowser"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestExternalBrowserNodeIntegration drives a real local browser through the
// Node: the public command, a session removal, a turn stopped in the middle
// of a browser call, and Node shutdown. It serves pages from loopback only.
func TestExternalBrowserNodeIntegration(t *testing.T) {
	if externalbrowser.Installed() == "" {
		t.Skip("no Chrome, Chromium or Edge is installed on this host; install one to run this test")
	}
	entered, release := make(chan struct{}, 1), make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.URL.Path == "/slow" {
			entered <- struct{}{}
			select {
			case <-release:
			case <-request.Context().Done():
			}
		}
		fmt.Fprint(writer, `<!doctype html><title>Fixture</title><h1 id="heading">Managed page</h1>`)
	}))
	defer server.Close()
	defer close(release)

	tool := func(name string) string { return operations.Alias("external-browser", name) }
	worker := &mediaScript{results: map[string]string{}, steps: [][][3]string{
		{{"discover", "tool_search", `{"query":"select:` + tool("browser_navigate") + `"}`}},
		{{"slow", tool("browser_navigate"), `{"url":"` + server.URL + `/slow"}`}},
	}}
	profile, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	profile = filepath.Join(profile, "profile")
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), profile, Options{
		Environment: &environment, Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return worker, nil },
		Process: process.Options{After: func(time.Duration) <-chan time.Time { return make(chan time.Time) }, Run: func(context.Context, process.Launch, func(protocol.ProcessOutputOutputStream, []byte), <-chan []byte) (protocol.ProcessCompletion, error) {
			return protocol.ProcessCompletion{}, errors.New("browser integration must not launch a process through the process service")
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	stopped := false
	defer func() {
		if !stopped {
			_ = node.Stop(context.Background())
		}
	}()
	processes := func() string {
		output, _ := exec.Command("pgrep", "-fl", "--", "--user-data-dir="+profile).Output()
		return strings.TrimSpace(string(output))
	}
	settled := func(what string, done func() bool) {
		t.Helper()
		for deadline := time.Now().Add(15 * time.Second); !done(); time.Sleep(50 * time.Millisecond) {
			if time.Now().After(deadline) {
				t.Fatalf("%s; sessions %v; processes:\n%s", what, node.externalBrowser.Sessions(), processes())
			}
		}
	}
	info := commandFixture(t, node, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: "external-browser"}}).Data.(protocol.PluginInfo)
	commandFixture(t, node, protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: "external-browser", ExpectedRevision: info.Summary.Revision, Enabled: true}})
	secret := protocol.NewSecret("browser-key")
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Browser fixture", true
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://127.0.0.1:1/v1", "worker"
	entry := protocol.DefaultConversationModel()
	entry.ID, entry.Context, entry.Output, entry.Tools, entry.CustomCapabilities = "worker", 32000, 8000, true, true
	provider.Models = append(provider.Models, entry)
	commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	create := func() protocol.Session {
		t.Helper()
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "worker", protocol.WorkModeCode, protocol.PermissionFull
		return commandFixture(t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &config}}).Data.(protocol.Session)
	}
	use := func(session protocol.Session, action protocol.ExternalBrowserAction, arguments string) map[string]any {
		t.Helper()
		output := commandFixture(t, node, protocol.Command{Kind: "use_external_browser", Data: &protocol.CommandUseExternalBrowserPayload{
			Session: session.ID, Worktree: session.Worktree, Action: action, Arguments: json.RawMessage(arguments)}})
		body, err := json.Marshal(output.Data)
		if err != nil {
			t.Fatal(err)
		}
		var data map[string]any
		if output.Kind != "external_browser" || json.Unmarshal(body, &data) != nil {
			t.Fatalf("output = %s %s", output.Kind, body)
		}
		return data
	}

	// The public command starts the session's browser and reads the page.
	first := create()
	use(first, protocol.ExternalBrowserActionNavigate, `{"url":"`+server.URL+`/"}`)
	text := use(first, protocol.ExternalBrowserActionExtractText, `{"selector":"#heading"}`)
	if result, _ := text["result"].(map[string]any); result["text"] != "Managed page" {
		t.Fatalf("extracted text = %v", text)
	}
	if sessions := node.externalBrowser.Sessions(); len(sessions) != 1 || sessions[0] != first.ID || processes() == "" {
		t.Fatalf("browser sessions = %v; processes:\n%s", sessions, processes())
	}
	// Removing the conversation closes its browser.
	commandFixture(t, node, protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: first.ID, ExpectedRevision: first.Revision}})
	settled("removed session kept its browser", func() bool { return len(node.externalBrowser.Sessions()) == 0 && processes() == "" })

	// Stopping a turn in the middle of a browser call closes the browser.
	second := create()
	commandFixture(t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: second.ID, ExpectedRevision: second.Revision, Message: protocol.ConversationInput{Text: "Open the slow page"},
	}})
	select {
	case <-entered:
	case <-time.After(60 * time.Second):
		t.Fatalf("turn never reached the browser; results %v", worker.results)
	}
	if processes() == "" {
		t.Fatal("turn reached the page without a managed browser process")
	}
	read := func() protocol.ConversationHistory {
		t.Helper()
		var history protocol.ConversationHistory
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(t.Context(), db, second.ID, nil, 100)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		return history
	}
	history := read()
	if len(history.Page.Runs) != 1 {
		t.Fatalf("runs = %+v", history.Page.Runs)
	}
	commandFixture(t, node, protocol.Command{Kind: "stop_turn", Data: protocol.CommandStopTurnPayload{Turn: history.Page.Runs[0].Turn}})
	settled("stopped turn kept its browser", func() bool {
		return read().Page.Runs[0].Status == protocol.ConversationStatusCancelled && len(node.externalBrowser.Sessions()) == 0 && processes() == ""
	})

	// Stopping the Node closes every remaining browser.
	third := create()
	use(third, protocol.ExternalBrowserActionNavigate, `{"url":"`+server.URL+`/"}`)
	if processes() == "" {
		t.Fatal("third session has no managed browser process")
	}
	stopped = true
	if err := node.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	if left := processes(); left != "" {
		t.Fatalf("browser processes survived Node stop:\n%s", left)
	}
	if entries, _ := os.ReadDir(filepath.Join(profile, "tmp")); len(entries) != 0 {
		t.Fatalf("Node stop left %d temporary entries", len(entries))
	}
}
