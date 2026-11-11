package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHostSDKCalls(t *testing.T) {
	t.Run("canonical card host", testCardHost)
	t.Run("native retained lifecycle", testNativeRetainedLifecycle)
	t.Run("host construction observations", testHostConstructionObservations)
	t.Run("request failure reconciliation", testHostRequestFailures)
	t.Run("a closed view stops its page fetch and code query", testHostReadCancellation)
	t.Run("usage sample", testHostUsageSample)
	t.Run("decimal revisions cross JavaScript exactly", func(t *testing.T) {
		for _, current := range []struct {
			value string
			valid bool
		}{
			{`"18446744073709551615"`, true},
			{`"9007199254740993"`, true},
			{`9007199254740991`, true},
			{`9007199254740992`, false},
			{`"01"`, false},
			{`"-1"`, false},
		} {
			t.Run(current.value, func(t *testing.T) {
				command, err := decodeHostCommand([]byte(`{"kind":"write_plugin_value","data":{"key":"draft","value":true,"expected_revision":` + current.value + `}}`))
				if (err == nil) != current.valid {
					t.Fatalf("revision acceptance changed: %v", err)
				}
				if err == nil {
					body, err := json.Marshal(command.Data)
					if err != nil || !strings.Contains(string(body), strings.Trim(current.value, `"`)) {
						t.Fatalf("revision changed during command capture: %s %v", body, err)
					}
				}
			})
		}
	})
	t.Run("captured drafts retain request and resource identity", func(t *testing.T) {
		scope := protocol.DefaultPluginContext()
		scope.Package = protocol.PluginReference{Name: "host-fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 9}
		worktree, session := protocol.NewWorktreeID(), protocol.NewSessionID()
		scope.Worktree, scope.Session = &worktree, &session
		transport := &hostTestTransport{target: protocol.NodeID{1}}
		controller := client.New(transport)
		defer controller.Close()
		owner := NewHost(HostBinding{Context: t.Context(), Client: controller, Scope: scope})
		defer owner.Close()
		command := protocol.Command{
			Kind: "write_plugin_value",
			Data: protocol.CommandWritePluginValuePayload{Key: "draft", Value: json.RawMessage(`true`), ExpectedRevision: 9},
		}
		prepared, err := owner.prepare(t.Context(), command)
		if err != nil {
			t.Fatal(err)
		}
		var identity string
		if err := json.Unmarshal(prepared, &identity); err != nil {
			t.Fatal(err)
		}
		owner.mu.Lock()
		owner.settings.current.SettingsRevision++
		owner.binding.Scope.Worktree = nil
		owner.mu.Unlock()
		for range 2 {
			if _, err := owner.complete(t.Context(), identity, false); err != nil {
				t.Fatal(err)
			}
		}
		requests := transport.snapshot()
		if len(requests) != 2 || requests[0].ID != requests[1].ID || !reflect.DeepEqual(requests[0].Plugin, &scope) || !reflect.DeepEqual(requests[0], requests[1]) {
			t.Fatalf("draft recovery replaced its captured identity: %#v", requests)
		}
		if _, err := owner.forget(t.Context(), identity); err != nil {
			t.Fatal(err)
		}
		if _, err := owner.complete(t.Context(), identity, false); err == nil {
			t.Fatal("forgotten request was dispatched")
		}
	})
	t.Run("protected output never enters script results", func(t *testing.T) {
		for _, output := range []protocol.Output{
			{Kind: "plugin_secret"},
			{Kind: "provider_key"},
			{Kind: "plugin_transaction", Data: []protocol.Output{{Kind: "plugin_secret"}}},
			{Kind: "plugin_transaction", Data: &[]protocol.Output{{Kind: "provider_key"}}},
		} {
			if _, err := publicHostOutput(output); err == nil {
				t.Fatalf("protected output was exposed: %s", output.Kind)
			}
		}
		value, err := publicHostOutput(protocol.Output{Kind: "plugin_value", Data: protocol.PluginStorageEntry{
			Key: "draft", Revision: 9_007_199_254_740_993, Present: true, Value: json.RawMessage(`true`),
		}})
		if err != nil || hostAt(value, "data", "revision") != "9007199254740993" {
			t.Fatalf("public revision lost precision: %#v %v", value, err)
		}
	})
	t.Run("credential handles disclose only field metadata", func(t *testing.T) {
		owner := NewHost(HostBinding{Context: t.Context()})
		defer owner.Close()
		binding := owner.native
		handle, err := binding.credentialOperation(t.Context(), "createSecret", []json.RawMessage{json.RawMessage(`{"label":"Password"}`)})
		if err != nil {
			t.Fatal(err)
		}
		var identity string
		if err := json.Unmarshal(handle, &identity); err != nil {
			t.Fatal(err)
		}
		binding.state.mu.Lock()
		binding.state.secrets[identity].value = "native-only-credential"
		binding.state.mu.Unlock()
		description, err := binding.credentialOperation(t.Context(), "describeSecret", []json.RawMessage{handle})
		if err != nil || strings.Contains(string(description), "native-only-credential") || !strings.Contains(string(description), `"filled":true`) {
			t.Fatalf("credential metadata leaked or lost field state: %s %v", description, err)
		}
		if _, err := binding.credentialOperation(t.Context(), "releaseSecret", []json.RawMessage{handle}); err != nil {
			t.Fatal(err)
		}
		if _, err := binding.secret(identity); err == nil {
			t.Fatal("released credential could still be consumed")
		}
		if err := owner.Close(); err != nil {
			t.Fatal(err)
		}
		if _, err := owner.prepare(t.Context(), protocol.Command{Kind: "list_plugins"}); err == nil {
			t.Fatal("closed Host accepted a new draft")
		}
	})
}

type hostTestTransport struct {
	mu       sync.Mutex
	target   protocol.NodeID
	requests []protocol.Request
}

func (transport *hostTestTransport) Target() protocol.NodeID {
	return transport.target
}

func (transport *hostTestTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	transport.mu.Lock()
	transport.requests = append(transport.requests, request)
	transport.mu.Unlock()
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: protocol.Output{Kind: "plugin_value", Data: protocol.PluginStorageEntry{
		Key: "draft", Revision: 10, Present: true, Value: json.RawMessage(`true`),
	}}}
	close(completion)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}, nil
}

func (transport *hostTestTransport) snapshot() []protocol.Request {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	return append([]protocol.Request(nil), transport.requests...)
}

func (transport *hostTestTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return nil, errors.New("fixture has no subscriptions")
}

func (transport *hostTestTransport) WatchConnection(ctx context.Context, _ func(link.ConnectionInfo)) error {
	<-ctx.Done()
	return ctx.Err()
}

func (transport *hostTestTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, errors.New("fixture has no streams")
}
