package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"net/http"
	"net/netip"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceHTTP func(*http.Request) (*http.Response, error)

func (f workspaceHTTP) RoundTrip(request *http.Request) (*http.Response, error) { return f(request) }

// Over 1 s: each Node start installs and verifies the bundled packages.
func TestWorkspaceAdmissionAndWebOwnership(t *testing.T) {
	t.Run("manual Git hunk receipt never replays its index effect", testManualGitHunkAdmission)
	t.Run("completed file effect never replays", func(t *testing.T) {
		node := nodeFixture(t)
		session := sessionFixture(t, node)
		root := ""
		for _, tree := range snapshotFixture(t, node).Worktrees {
			if tree.ID == session.Worktree {
				root = tree.Path
			}
		}
		if root == "" {
			t.Fatal("session workspace is missing")
		}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: "note.txt", Text: "first\n"}})
		first := dispatchFixture(t, node, node.ID(), request)
		if first.Fault != nil {
			t.Fatal(first.Fault)
		}
		if err := os.WriteFile(filepath.Join(root, "note.txt"), []byte("external\n"), 0600); err != nil {
			t.Fatal(err)
		}
		replay := dispatchFixture(t, node, node.ID(), request)
		before, _ := json.Marshal(first)
		after, _ := json.Marshal(replay)
		if !bytes.Equal(before, after) {
			t.Fatalf("receipt changed: %s %s", before, after)
		}
		data, err := os.ReadFile(filepath.Join(root, "note.txt"))
		if err != nil || string(data) != "external\n" {
			t.Fatalf("effect replayed: %q %v", data, err)
		}
		expected := "0000000000000000000000000000000000000000000000000000000000000000"
		conflict := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: "note.txt", Text: "overwrite", ExpectedRevision: &expected}}))
		if conflict.Fault == nil || conflict.Fault.Code != protocol.ErrorCodeRevisionConflict {
			t.Fatalf("conflict lost: %v", conflict.Fault)
		}
	})
	t.Run("a result that could not be stored is settled", func(t *testing.T) {
		node := nodeFixture(t)
		session := sessionFixture(t, node)
		ctx := context.Background()
		// The writer's connection refuses to store a result until the gate opens.
		gate := func(statement string) {
			t.Helper()
			if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
				_, err := db.ExecContext(ctx, statement)
				return err
			}); err != nil {
				t.Fatal(err)
			}
		}
		sandbox := commandFixture(t, node, protocol.Command{Kind: "read_sandbox_settings"}).Data.(protocol.SandboxSettings)
		gate("CREATE TEMP TABLE result_gate AS SELECT 1 AS closed")
		gate("CREATE TEMP TRIGGER result_refused BEFORE UPDATE OF status ON requests WHEN (SELECT closed FROM result_gate)=1 BEGIN SELECT RAISE(ABORT,'result write refused'); END")
		defer gate("DROP TRIGGER result_refused")
		// A file write has only its result to store; a settings save has its
		// whole change in the transaction that fails.
		requests := map[string]protocol.Request{
			"effect":   protocol.NewRequest(node.ID(), protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: session.Worktree, Path: "settled.txt", Text: "written\n"}}),
			"settings": protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_sandbox_settings", Data: &sandbox}),
		}
		inspect := func(name string) protocol.RequestOutcome {
			t.Helper()
			body, err := json.Marshal(requests[name])
			if err != nil {
				t.Fatal(err)
			}
			query := protocol.Command{Kind: "inspect_request", Data: protocol.CommandInspectRequestPayload{ID: requests[name].ID, Digest: admission.Digest(body)}}
			return commandFixture(t, node, query).Data.(protocol.OutputRequestOutcomePayload).Outcome
		}
		pending := map[string]<-chan link.Response{}
		for name, request := range requests {
			accepted, err := node.Dispatch(ctx, node.ID(), request)
			if err != nil {
				t.Fatal(err)
			}
			pending[name] = accepted.Completion
		}
		time.Sleep(200 * time.Millisecond)
		for name, completion := range pending {
			select {
			case early := <-completion:
				t.Fatalf("%s answered before its result was stored: %#v, %v", name, early.Output, early.Fault)
			default:
			}
			if outcome := inspect(name); outcome.Kind != "admitted" {
				t.Fatalf("%s while its result is unrecorded = %#v", name, outcome)
			}
		}
		gate("UPDATE result_gate SET closed=0")
		for name, completion := range pending {
			result := <-completion
			outcome := inspect(name)
			stored, ok := outcome.Data.(protocol.Result[protocol.Output, protocol.Fault])
			if outcome.Kind != "completed" || !ok {
				t.Fatalf("%s stored outcome = %#v", name, outcome)
			}
			replay := dispatchFixture(t, node, node.ID(), requests[name])
			switch name {
			case "effect":
				if result.Fault != nil || stored.Ok == nil || replay.Fault != nil {
					t.Fatalf("effect = %v, stored %#v, replay %v", result.Fault, stored, replay.Fault)
				}
			case "settings":
				if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeInternal || stored.Err == nil || *stored.Err != *result.Fault || replay.Fault == nil || *replay.Fault != *result.Fault {
					t.Fatalf("settings = %v, stored %#v, replay %v", result.Fault, stored, replay.Fault)
				}
			}
		}
		if saved := commandFixture(t, node, protocol.Command{Kind: "read_sandbox_settings"}).Data.(protocol.SandboxSettings); saved.Revision != sandbox.Revision {
			t.Fatalf("a failed save was applied: revision %d, was %d", saved.Revision, sandbox.Revision)
		}
	})
	t.Run("saved code index setting applies before completion", func(t *testing.T) {
		node := nodeFixture(t)
		session := sessionFixture(t, node)
		commandFixture(t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: protocol.CodeIndexSettings{Enabled: false}}})
		result := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: session.Worktree, Query: protocol.CodeIndexSearch{Query: "main", Limit: 1}}}))
		if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeNotConfigured {
			t.Fatalf("search after disabling = %#v", result)
		}
	})
	t.Run("caller scoped cancellation and public fetch boundary", func(t *testing.T) {
		started := make(chan struct{})
		cancelled := make(chan struct{})
		environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
		node, err := StartWithOptions(context.Background(), filepath.Join(t.TempDir(), "profile"), Options{Environment: &environment, Fetch: web.FetchOptions{Resolve: func(context.Context, string) ([]netip.Addr, error) {
			return []netip.Addr{netip.MustParseAddr("93.184.216.34")}, nil
		}, LocalAddresses: func() ([]netip.Addr, error) { return nil, nil }, Transport: workspaceHTTP(func(request *http.Request) (*http.Response, error) {
			close(started)
			<-request.Context().Done()
			close(cancelled)
			return nil, request.Context().Err()
		})}})
		if err != nil {
			t.Fatal(err)
		}
		defer node.Stop(context.Background())
		caller := node.ID()
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "fetch_web", Data: protocol.WebRequest{Url: "https://example.com/"}})
		accepted, err := node.Dispatch(context.Background(), caller, request)
		if err != nil {
			t.Fatal(err)
		}
		<-started
		other := protocol.NodeID{}
		other[0] = 1
		dispatchFixture(t, node, other, protocol.NewRequest(node.ID(), protocol.Command{Kind: "cancel_web_fetch", Data: protocol.CommandCancelWebFetchPayload{Request: request.ID}}))
		select {
		case <-cancelled:
			t.Fatal("another caller cancelled the request")
		default:
		}
		dispatchFixture(t, node, caller, protocol.NewRequest(node.ID(), protocol.Command{Kind: "cancel_web_fetch", Data: protocol.CommandCancelWebFetchPayload{Request: request.ID}}))
		result := <-accepted.Completion
		if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeCancelled {
			t.Fatalf("cancelled fetch = %#v", result)
		}
		result = dispatchFixture(t, node, caller, protocol.NewRequest(node.ID(), protocol.Command{Kind: "fetch_web", Data: protocol.WebRequest{Url: "http://127.0.0.1/"}}))
		if result.Fault == nil {
			t.Fatal("loopback fetch was accepted")
		}
	})
}
