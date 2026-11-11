package plugins

import (
	"context"
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testHostRequestFailures(t *testing.T) {
	t.Run("definite rejections preserve faults and unlock settings drafts", func(t *testing.T) {
		for _, code := range []protocol.ErrorCode{
			protocol.ErrorCodeInvalidRequest, protocol.ErrorCodeWrongTarget, protocol.ErrorCodeNotFound,
			protocol.ErrorCodeNotConfigured, protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict,
			protocol.ErrorCodePermissionDenied, protocol.ErrorCodeExpired,
		} {
			t.Run(string(code), func(t *testing.T) {
				failure := protocol.Fault{Code: code, Message: "settings request was rejected"}
				owner, transport := newHostRequestFixture(t, func(request protocol.Request) (link.Admission, error) {
					if request.Command.Kind == "inspect_request" {
						input := request.Command.Data.(protocol.CommandInspectRequestPayload)
						return hostRequestResponse(request, protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{
							ID: input.ID, Outcome: protocol.RequestOutcome{Kind: "not_admitted"},
						}}), nil
					}
					return link.Admission{}, &link.FaultError{Fault: failure}
				})
				identity := prepareHostRequestSettings(t, owner)
				result, err := owner.complete(t.Context(), identity, true)
				var outcome struct {
					Err *protocol.Fault `json:"Err"`
				}
				if err != nil || json.Unmarshal(result, &outcome) != nil || !reflect.DeepEqual(outcome.Err, &failure) {
					t.Fatalf("definite fault was replaced: %s %v", result, err)
				}
				if requests := transport.snapshot(); len(requests) != 1 || requests[0].Command.Kind != "save_plugin_settings" {
					t.Fatalf("definite rejection queried an uncertain outcome: %+v", requests)
				}
				owner.mu.Lock()
				pending := owner.settings.pending
				owner.mu.Unlock()
				if pending != nil {
					t.Fatal("definite rejection retained a pending settings save")
				}
				if _, err := owner.forget(t.Context(), identity); err != nil {
					t.Fatal(err)
				}
				prepareHostRequestSettings(t, owner)
			})
		}
	})
	t.Run("uncertain faults reconcile the captured request", func(t *testing.T) {
		for _, code := range []protocol.ErrorCode{
			protocol.ErrorCodeOutcomeUnknown, protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy,
			protocol.ErrorCodeInternal, protocol.ErrorCodeCancelled,
		} {
			t.Run(string(code), func(t *testing.T) {
				owner, transport := newHostRequestFixture(t, func(request protocol.Request) (link.Admission, error) {
					if request.Command.Kind != "inspect_request" {
						return link.Admission{}, &link.FaultError{Fault: protocol.Fault{Code: code, Message: "settings completion was lost"}}
					}
					input := request.Command.Data.(protocol.CommandInspectRequestPayload)
					saved := protocol.DefaultPluginSettingsState()
					saved.Package = hostRequestReference()
					saved.Package.SettingsRevision++
					saved.Values["draft"] = json.RawMessage(`"retained"`)
					output := protocol.Output{Kind: "plugin_settings", Data: saved}
					return hostRequestResponse(request, protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{
						ID: input.ID, Outcome: protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}},
					}}), nil
				})
				identity := prepareHostRequestSettings(t, owner)
				body, err := owner.complete(t.Context(), identity, true)
				outcome, decodeErr := decodeHostJSON(body)
				if err != nil || decodeErr != nil || hostAt(outcome, "Ok", "kind") != "plugin_settings" ||
					hostAt(outcome, "Ok", "data", "package", "settings_revision") != "2" {
					t.Fatalf("confirmed settings completion was lost: %s %v %v", body, err, decodeErr)
				}
				requests := transport.snapshot()
				if len(requests) != 2 || requests[1].Command.Kind != "inspect_request" {
					t.Fatalf("uncertain completion did not inspect the captured request: %+v", requests)
				}
				input := requests[1].Command.Data.(protocol.CommandInspectRequestPayload)
				if input.ID != requests[0].ID {
					t.Fatal("reconciliation changed the captured request identity")
				}
				owner.mu.Lock()
				pending, current := owner.settings.pending, owner.settings.current
				owner.mu.Unlock()
				if pending != nil || current.SettingsRevision != 2 {
					t.Fatal("confirmed completion did not settle settings state")
				}
			})
		}
	})
	t.Run("unknown outcome retains the draft for same-ID retry", func(t *testing.T) {
		outcomes := 0
		owner, transport := newHostRequestFixture(t, func(request protocol.Request) (link.Admission, error) {
			if request.Command.Kind != "inspect_request" {
				return link.Admission{}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "settings completion was lost"}}
			}
			input := request.Command.Data.(protocol.CommandInspectRequestPayload)
			outcomes++
			outcome := protocol.RequestOutcome{Kind: "unknown"}
			if outcomes == 2 {
				failure := protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "settings changed before the save"}
				outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Err: &failure}}
			}
			return hostRequestResponse(request, protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: input.ID, Outcome: outcome}}), nil
		})
		identity := prepareHostRequestSettings(t, owner)
		if _, err := owner.complete(t.Context(), identity, true); err == nil || err.Error() != "unconfirmed" {
			t.Fatalf("unknown completion was treated as settled: %v", err)
		}
		owner.mu.Lock()
		pending := owner.settings.pending
		owner.mu.Unlock()
		if pending == nil || pending.identity.String() != identity {
			t.Fatal("unknown completion discarded its pending settings identity")
		}
		if _, err := owner.forget(t.Context(), identity); err == nil {
			t.Fatal("unknown settings request could be forgotten")
		}
		body, err := owner.complete(t.Context(), identity, true)
		outcome, decodeErr := decodeHostJSON(body)
		if err != nil || decodeErr != nil || hostAt(outcome, "Err", "code") != string(protocol.ErrorCodeRevisionConflict) {
			t.Fatalf("retry did not return the original confirmed conflict: %s %v %v", body, err, decodeErr)
		}
		requests := transport.snapshot()
		if len(requests) != 4 || !reflect.DeepEqual(requests[0], requests[2]) {
			t.Fatalf("retry changed the captured settings request: %+v", requests)
		}
		first := requests[1].Command.Data.(protocol.CommandInspectRequestPayload)
		second := requests[3].Command.Data.(protocol.CommandInspectRequestPayload)
		if first != second {
			t.Fatal("retry inspected a different request identity or body")
		}
		if _, err := owner.forget(t.Context(), identity); err != nil {
			t.Fatal(err)
		}
		prepareHostRequestSettings(t, owner)
	})
}

// testHostReadCancellation covers a page fetch and a code index query whose
// view closes: the Node is told to stop that request, under the same scope.
func testHostReadCancellation(t *testing.T) {
	for _, current := range []struct {
		kind, cancel string
		data         map[string]any
		stopped      func(any) protocol.RequestID
	}{
		{"fetch_web", "cancel_web_fetch", map[string]any{"url": "https://page.example.test/"}, func(data any) protocol.RequestID {
			return data.(protocol.CommandCancelWebFetchPayload).Request
		}},
		{"outline_code", "cancel_code_index_query", map[string]any{"worktree": protocol.NewWorktreeID().String(), "path": "main.go"}, func(data any) protocol.RequestID {
			return data.(protocol.CommandCancelCodeIndexQueryPayload).Request
		}},
	} {
		t.Run(current.kind, func(t *testing.T) {
			admitted := make(chan struct{})
			owner, transport := newHostRequestFixture(t, func(request protocol.Request) (link.Admission, error) {
				if request.Command.Kind == current.cancel {
					return hostRequestResponse(request, protocol.Output{}), nil
				}
				// The answer never arrives.
				close(admitted)
				return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: make(chan link.Response)}, nil
			})
			ctx, cancel := context.WithCancel(t.Context())
			finished := make(chan error, 1)
			go func() {
				_, err := owner.readData(ctx, current.kind, current.data)
				finished <- err
			}()
			<-admitted
			cancel()
			if err := <-finished; err == nil {
				t.Fatal("a read whose view closed returned a value")
			}
			requests := transport.snapshot()
			if len(requests) != 2 || requests[0].Command.Kind != current.kind || requests[1].Command.Kind != current.cancel {
				t.Fatalf("requests sent: %+v", requests)
			}
			if stopped := current.stopped(requests[1].Command.Data); stopped != requests[0].ID || requests[1].Plugin == nil || !reflect.DeepEqual(requests[1].Plugin, requests[0].Plugin) {
				t.Fatalf("cancelled %v under %+v, the read was %v under %+v", stopped, requests[1].Plugin, requests[0].ID, requests[0].Plugin)
			}
		})
	}
}

func hostRequestReference() protocol.PluginReference {
	return protocol.PluginReference{Name: "request-fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 1}
}

func newHostRequestFixture(t *testing.T, dispatch func(protocol.Request) (link.Admission, error)) (*Host, *hostRequestTransport) {
	t.Helper()
	reference := hostRequestReference()
	plugin := protocol.DefaultPluginSummary()
	plugin.Name, plugin.Digest, plugin.SettingsRevision = reference.Name, reference.Digest, reference.SettingsRevision
	plugin.Revision, plugin.Enabled = 1, true
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{plugin}
	transport := &hostRequestTransport{
		hostTestTransport: hostTestTransport{target: snapshot.Node},
		subscription:      newHostConstructionSubscription(snapshot), dispatch: dispatch,
	}
	controller := client.New(transport)
	t.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Surface.Kind = reference, "settings"
	owner := NewHost(HostBinding{Context: t.Context(), Client: controller, Scope: scope})
	t.Cleanup(func() { _ = owner.Close() })
	return owner, transport
}

func prepareHostRequestSettings(t *testing.T, owner *Host) string {
	t.Helper()
	prepared, err := owner.prepare(t.Context(), protocol.Command{Kind: "save_plugin_settings", Data: protocol.CommandSavePluginSettingsPayload{
		Package: owner.scope().Package, Values: map[string]json.RawMessage{"draft": json.RawMessage(`"retained"`)},
		Secrets: map[string]protocol.PluginSettingsSecretUpdate{},
	}})
	var identity string
	if err != nil || json.Unmarshal(prepared, &identity) != nil {
		t.Fatalf("prepare settings save: %s %v", prepared, err)
	}
	return identity
}

func hostRequestResponse(request protocol.Request, output protocol.Output) link.Admission {
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: output}
	close(completion)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
}

type hostRequestTransport struct {
	hostTestTransport
	subscription link.Subscription
	dispatch     func(protocol.Request) (link.Admission, error)
}

func (transport *hostRequestTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	transport.mu.Lock()
	transport.requests = append(transport.requests, request)
	transport.mu.Unlock()
	return transport.dispatch(request)
}

func (transport *hostRequestTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return transport.subscription, nil
}
