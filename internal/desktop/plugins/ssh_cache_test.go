package plugins

import (
	"context"
	"encoding/json"
	"fmt"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type sshCacheTestTransport struct {
	target   protocol.NodeID
	dispatch func(protocol.Request) (link.Admission, error)
}

func (t *sshCacheTestTransport) Target() protocol.NodeID { return t.target }
func (t *sshCacheTestTransport) Dispatch(_ context.Context, req protocol.Request) (link.Admission, error) {
	return t.dispatch(req)
}
func (t *sshCacheTestTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return nil, hostUnavailable("unused")
}
func (t *sshCacheTestTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, hostUnavailable("unused")
}
func (t *sshCacheTestTransport) WatchConnection(ctx context.Context, _ func(link.ConnectionInfo)) error {
	<-ctx.Done()
	return ctx.Err()
}

func TestSshDirectoryCacheAcrossRemountRefreshAndMutation(t *testing.T) {
	t.Parallel()
	clock := clockwork.NewFakeClock()
	cache := NewSshDirectoryCache(clock)
	profileID := protocol.NewSshID()
	target := protocol.NodeID{4}

	var browses atomic.Int32
	var hostKeyMode atomic.Bool
	transport := &sshCacheTestTransport{
		target: target,
		dispatch: func(request protocol.Request) (link.Admission, error) {
			switch request.Command.Kind {
			case "browse_ssh_directory":
				count := browses.Add(1)
				if hostKeyMode.Load() {
					return hostRequestResponse(request, protocol.Output{
						Kind: "ssh_outcome",
						Data: protocol.SshOutcome{
							Kind: "host_key_required",
							Data: protocol.SshOutcomeHostKeyRequiredPayload{
								Key: protocol.SshHostKey{Algorithm: "ssh-ed25519", Fingerprint: "SHA256:test"},
							},
						},
					}), nil
				}
				return hostRequestResponse(request, protocol.Output{
					Kind: "ssh_outcome",
					Data: protocol.SshOutcome{
						Kind: "directory",
						Data: protocol.SshDirectory{
							Path: "/root",
							Entries: []protocol.FileEntry{
								{Name: fmt.Sprintf("file-%d.txt", count), Kind: protocol.EntryKindFile},
							},
						},
					},
				}), nil
			case "modify_ssh_file":
				return hostRequestResponse(request, protocol.Output{
					Kind: "ssh_outcome",
					Data: protocol.SshOutcome{Kind: "files_changed"},
				}), nil
			default:
				return link.Admission{}, fmt.Errorf("unexpected command %s", request.Command.Kind)
			}
		},
	}
	controller := client.New(transport)
	defer controller.Close()

	newMountedHost := func() *Host {
		scope := protocol.DefaultPluginContext()
		scope.Package = protocol.PluginReference{Name: "ssh", Digest: "0000000000000000000000000000000000000000000000000000000000000001", SettingsRevision: 1}
		return NewHost(HostBinding{
			Context:           t.Context(),
			Client:            controller,
			Target:            target,
			Scope:             scope,
			Clock:             clock,
			SshDirectoryCache: cache,
		})
	}
	browse := func(owner *Host, path string, revision uint64) string {
		t.Helper()
		arg := json.RawMessage(fmt.Sprintf(`{"profile":%q,"expected_revision":%d,"path":%q,"after":null}`, profileID.String(), revision, path))
		for _, export := range owner.connectionModule().Exports {
			if export.Name == "browseSshDirectory" {
				body, err := export.Async(t.Context(), []json.RawMessage{arg})
				if err != nil {
					t.Fatalf("browseSshDirectory(%q): %v", path, err)
				}
				return string(body)
			}
		}
		t.Fatal("browseSshDirectory export missing")
		return ""
	}

	// Mount 1 loads "." once and populates both "." and resolved "/root".
	first := newMountedHost()
	body1 := browse(first, ".", 1)
	if browses.Load() != 1 {
		t.Fatalf("first browse count = %d, want 1", browses.Load())
	}
	// Close mount 1 (simulating panel switch away from SSH).
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}

	// Mount 2 (simulating switching back to SSH) hits cache for "." and "/root" with 0 RPCs.
	second := newMountedHost()
	defer second.Close()
	if body2 := browse(second, ".", 1); body2 != body1 || browses.Load() != 1 {
		t.Fatalf("remount browse did not reuse cache: count=%d body=%s", browses.Load(), body2)
	}
	if bodyRoot := browse(second, "/root", 1); bodyRoot != body1 || browses.Load() != 1 {
		t.Fatalf("resolved /root did not reuse cache: count=%d body=%s", browses.Load(), bodyRoot)
	}

	// Explicit refresh button event invalidates cache immediately before reload.
	second.native.emit("control", map[string]any{"id": "ssh-files-refresh"})
	if body3 := browse(second, "/root", 1); body3 == body1 || browses.Load() != 2 {
		t.Fatalf("explicit refresh did not bypass cache: count=%d body=%s", browses.Load(), body3)
	}

	// Right-click tree menu refresh invalidates cache for that profile.
	menuAction := fmt.Sprintf(`{"action":"refresh","path":"/root","profile":%q,"revision":1,"generation":1}`, profileID.String())
	second.native.emit("tree", map[string]any{"tree": "ssh-files", "id": "", "kind": "menu", "action": menuAction})
	if browse(second, "/root", 1); browses.Load() != 3 {
		t.Fatalf("tree menu refresh did not invalidate cache: count=%d", browses.Load())
	}

	// File mutation invalidates cache on completion.
	prepared, err := second.prepareData(context.Background(), "modify_ssh_file", map[string]any{
		"profile": profileID.String(), "expected_revision": 1, "path": "/root/new.txt",
		"action": map[string]any{"operation": "create", "directory": false},
	})
	if err != nil {
		t.Fatal(err)
	}
	var reqID string
	if err := json.Unmarshal(prepared, &reqID); err != nil {
		t.Fatal(err)
	}
	if _, err := second.complete(t.Context(), reqID, true); err != nil {
		t.Fatal(err)
	}
	if browse(second, "/root", 1); browses.Load() != 4 {
		t.Fatalf("modify_ssh_file did not invalidate cache: count=%d", browses.Load())
	}

	// Revision bump misses old cached revision and evicts it.
	if browse(second, ".", 2); browses.Load() != 5 {
		t.Fatalf("revision bump did not miss cache: count=%d", browses.Load())
	}

	// TTL expiry evicts stale entries.
	clock.Advance(sshDirectoryCacheTTL + time.Second)
	if browse(second, ".", 2); browses.Load() != 6 {
		t.Fatalf("expired entry did not reload: count=%d", browses.Load())
	}

	// host_key_required responses must never be cached.
	cache.Clear()
	hostKeyMode.Store(true)
	_ = browse(second, ".", 2)
	if cache.Len() != 0 {
		t.Fatalf("host_key_required was cached: len=%d", cache.Len())
	}
}
