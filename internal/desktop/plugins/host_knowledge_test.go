package plugins

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type hostKnowledgeTransport struct {
	*hostTestTransport
	entry    protocol.NodeID
	requests *[]protocol.Request
}

type hostScopedKnowledgeTransport struct {
	hostKnowledgeTransport
	scoped *[]protocol.ScopedKnowledgeCall
}

func (transport hostScopedKnowledgeTransport) ScopedKnowledge(_ context.Context, call protocol.ScopedKnowledgeCall) (link.Admission, error) {
	*transport.scoped = append(*transport.scoped, call)
	return hostRequestResponse(call.Request, protocol.Output{Kind: "knowledge_collections", Data: []protocol.KnowledgeCollection{}}), nil
}

func (transport hostKnowledgeTransport) Route() protocol.RouteKey {
	return protocol.RouteKey{Entry: transport.entry, Target: transport.target}
}
func (transport hostKnowledgeTransport) ForTarget(target protocol.NodeID) link.Transport {
	transport.hostTestTransport = &hostTestTransport{target: target}
	return transport
}
func (transport hostKnowledgeTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	*transport.requests = append(*transport.requests, request)
	return hostRequestResponse(request, protocol.Output{Kind: "knowledge_collections", Data: []protocol.KnowledgeCollection{}}), nil
}

func TestHostKnowledgeSelectsStorageBeforePreparing(t *testing.T) {
	entry, storage := protocol.NodeID{1}, protocol.NodeID{2}
	var requests []protocol.Request
	controller := client.New(hostKnowledgeTransport{hostTestTransport: &hostTestTransport{target: entry}, entry: entry, requests: &requests})
	t.Cleanup(func() { _ = controller.Close() })
	owner := NewHost(HostBinding{Context: t.Context(), Client: controller, Scope: protocol.DefaultPluginContext()})
	t.Cleanup(func() { _ = owner.Close() })
	reference := protocol.KnowledgeRef{Node: storage, ID: protocol.NewKnowledgeID()}
	encoded, err := json.Marshal(reference)
	if err != nil {
		t.Fatal(err)
	}
	prepared, err := owner.prepareKnowledgeChange("delete_knowledge_collection", "expected_revision")(t.Context(), []json.RawMessage{encoded, json.RawMessage(`"9007199254740993"`)})
	var identity string
	if err != nil || json.Unmarshal(prepared, &identity) != nil {
		t.Fatalf("prepare collection removal: %s: %v", prepared, err)
	}
	request, err := owner.prepared(t.Context(), identity)
	if err != nil || request.Target != storage {
		t.Fatalf("wrong prepared storage Node: %+v: %v", request, err)
	}
	if _, err := owner.complete(t.Context(), identity, false); err != nil {
		t.Fatal(err)
	}
	if len(requests) != 1 || requests[0].ID != request.ID || requests[0].Target != storage {
		t.Fatalf("completed a different request: %+v", requests)
	}
	payload, valid := requests[0].Command.Data.(*protocol.CommandDeleteKnowledgeCollectionPayload)
	if !valid || payload.ID != reference.ID || payload.Node == nil || *payload.Node != storage || payload.ExpectedRevision != 9_007_199_254_740_993 {
		t.Fatalf("collection reference or revision was changed: %+v", requests[0])
	}
}

func TestHostWorkspaceKnowledgeValidatesAtItsExecutionNode(t *testing.T) {
	entry, execution := protocol.NodeID{1}, protocol.NodeID{2}
	var requests []protocol.Request
	var scoped []protocol.ScopedKnowledgeCall
	controller := client.New(hostKnowledgeTransport{hostTestTransport: &hostTestTransport{target: entry}, entry: entry, requests: &requests})
	workspace := client.New(hostScopedKnowledgeTransport{
		hostKnowledgeTransport: hostKnowledgeTransport{hostTestTransport: &hostTestTransport{target: execution}, entry: entry, requests: &requests},
		scoped:                 &scoped,
	})
	t.Cleanup(func() { _ = controller.Close(); _ = workspace.Close() })
	worktree, session := protocol.NewWorktreeID(), protocol.NewSessionID()
	scope := protocol.DefaultPluginContext()
	scope.Worktree, scope.Session = &worktree, &session
	owner := NewHost(HostBinding{Context: t.Context(), Client: workspace, Resources: controller, Scope: scope})
	t.Cleanup(func() { _ = owner.Close() })
	query := protocol.DefaultKnowledgeQuery()
	query.Query = "notes"
	if _, err := owner.read(t.Context(), protocol.Command{Kind: "query_knowledge", Data: &query}); err != nil {
		t.Fatal(err)
	}
	if len(requests) != 0 || len(scoped) != 1 || scoped[0].Request.Target != entry || scoped[0].Request.Plugin == nil || scoped[0].Request.Plugin.Session == nil || *scoped[0].Request.Plugin.Session != session {
		t.Fatalf("workspace scope bypassed its execution Node: ordinary=%+v scoped=%+v", requests, scoped)
	}
	definition := protocol.DefaultKnowledgeDefinition()
	definition.Source = protocol.KnowledgeSource{Kind: "storage", Data: &protocol.KnowledgeSourceStoragePayload{Scope: protocol.PluginStorageScopeNode}}
	created := owner.prepareHostCommand(protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}})
	if created.Target != entry {
		t.Fatalf("personal knowledge targeted execution Node instead of controller: %+v", created)
	}
}

func TestHostWorkspaceKnowledgeReferenceUsesExecutionScope(t *testing.T) {
	entry, execution, storage := protocol.NodeID{1}, protocol.NodeID{2}, protocol.NodeID{3}
	var ordinary []protocol.Request
	var scoped []protocol.ScopedKnowledgeCall
	controller := client.New(hostKnowledgeTransport{hostTestTransport: &hostTestTransport{target: entry}, entry: entry, requests: &ordinary})
	workspace := client.New(hostScopedKnowledgeTransport{
		hostKnowledgeTransport: hostKnowledgeTransport{hostTestTransport: &hostTestTransport{target: execution}, entry: entry, requests: &ordinary},
		scoped:                 &scoped,
	})
	t.Cleanup(func() { _ = controller.Close(); _ = workspace.Close() })
	worktree, session := protocol.NewWorktreeID(), protocol.NewSessionID()
	scope := protocol.DefaultPluginContext()
	scope.Worktree, scope.Session = &worktree, &session
	owner := NewHost(HostBinding{Context: t.Context(), Client: workspace, Resources: controller, Scope: scope})
	t.Cleanup(func() { _ = owner.Close() })
	request := owner.prepareHostCommand(protocol.Command{Kind: "delete_knowledge_collection", Data: protocol.CommandDeleteKnowledgeCollectionPayload{Node: &storage, ID: protocol.NewKnowledgeID(), ExpectedRevision: 1}})
	request.Plugin = &scope
	if _, err := owner.executeHostCommand(t.Context(), request); err != nil {
		t.Fatal(err)
	}
	if len(ordinary) != 0 || len(scoped) != 1 || scoped[0].Request.Target != storage || scoped[0].Request.ID != request.ID ||
		scoped[0].Request.Plugin == nil || scoped[0].Request.Plugin.Session == nil || *scoped[0].Request.Plugin.Session != session {
		t.Fatalf("workspace request bypassed its execution scope: ordinary=%+v scoped=%+v", ordinary, scoped)
	}
}
