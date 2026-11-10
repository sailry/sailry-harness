package node

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/netip"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceHTTP func(*http.Request) (*http.Response, error)

func (f workspaceHTTP) RoundTrip(request *http.Request) (*http.Response, error) { return f(request) }
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
