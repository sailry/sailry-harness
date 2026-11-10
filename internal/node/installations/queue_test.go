package installations

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/tasks"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestInstallQueueOrder(t *testing.T) {
	t.Run("scoped snapshots validate IDs and preserve captured authority", func(t *testing.T) {
		node := protocol.NodeID{1}
		serviceClosed := make(chan struct{})
		queue := New(nil, node, serviceClosed)
		scope := protocol.DefaultPluginContext()
		scope.Package = protocol.PluginReference{Name: "private-owner", Digest: strings.Repeat("a", 64)}
		worktree, storedWorktree := protocol.NewWorktreeID(), protocol.WorktreeID{}
		storedWorktree = worktree
		scope.Worktree = &worktree
		captured := scope
		captured.Worktree = &storedWorktree
		foreign := captured
		foreign.Package.SettingsRevision = 1
		first, second, other, packageID, foreignCaller := protocol.NewRequestID(), protocol.NewRequestID(), protocol.NewRequestID(), protocol.NewRequestID(), protocol.NewRequestID()
		model := protocol.InstallationResource{Kind: "model", Data: protocol.InstallationResourceModelPayload{Kind: protocol.ModelsKindOcr, ID: "fixture"}}
		language := protocol.InstallationResource{Kind: "language", Data: protocol.InstallationResourceLanguagePayload{Package: "fixture", Version: "0.1.0"}}
		queue.records[key{node, first}] = &record{task: protocol.InstallationTask{Request: first, Revision: 1, Resource: model, State: protocol.InstallationState{Kind: "queued"}}, plugin: &captured, position: 1}
		queue.records[key{node, second}] = &record{task: protocol.InstallationTask{Request: second, Revision: 1, Resource: language, State: protocol.InstallationState{Kind: "queued"}}, plugin: &captured, position: 2}
		queue.records[key{node, other}] = &record{task: protocol.InstallationTask{Request: other, Revision: 1, Resource: model, State: protocol.InstallationState{Kind: "queued"}}, plugin: &foreign, position: 3}
		queue.records[key{node, packageID}] = &record{task: protocol.InstallationTask{Request: packageID, Revision: 1, Resource: protocol.InstallationResource{Kind: "plugin", Data: protocol.InstallationResourcePluginPayload{Name: "fixture"}}, State: protocol.InstallationState{Kind: "queued"}}, plugin: &captured, position: 4}
		queue.records[key{protocol.NodeID{2}, foreignCaller}] = &record{task: protocol.InstallationTask{Request: foreignCaller, Revision: 1, Resource: model, State: protocol.InstallationState{Kind: "queued"}}, plugin: &captured, position: 5}
		assertFault := func(err error, code protocol.ErrorCode) {
			t.Helper()
			var failed *protocol.Fault
			if !errors.As(err, &failed) || failed.Code != code {
				t.Fatalf("scoped installation fault = %v; want %s", err, code)
			}
		}
		for _, requests := range [][]protocol.RequestID{nil, {first, first}, make([]protocol.RequestID, 65)} {
			_, err := queue.SubscribeFor(node, requests, scope)
			assertFault(err, protocol.ErrorCodeInvalidRequest)
			_, err = queue.Read(node, requests, &scope)
			assertFault(err, protocol.ErrorCodeInvalidRequest)
		}
		for _, request := range []protocol.RequestID{other, packageID} {
			_, err := queue.SubscribeFor(node, []protocol.RequestID{request}, scope)
			assertFault(err, protocol.ErrorCodePermissionDenied)
		}
		requests := []protocol.RequestID{second, first, foreignCaller}
		stream, err := queue.SubscribeFor(node, requests, scope)
		if err != nil {
			t.Fatal(err)
		}
		defer stream.Close()
		requests[0] = other
		worktree = protocol.NewWorktreeID()
		update, err := stream.Next(t.Context())
		if err != nil {
			t.Fatal(err)
		}
		value := update.Data.(protocol.UpdateInstallationsPayload)
		if update.Kind != "installations" || value.Node != node || len(value.Tasks) != 2 || value.Tasks[0].Request != first || value.Tasks[1].Request != second {
			t.Fatal("scoped installation snapshot lost captured IDs, caller, scope or admission order")
		}
		body, err := json.Marshal(update)
		if err != nil || strings.Contains(string(body), "private-owner") || strings.Contains(string(body), captured.Package.Digest) {
			t.Fatal("private plugin authority leaked through an installation snapshot")
		}
		queue.mu.Lock()
		queue.records[key{node, first}].plugin = &foreign
		queue.publish()
		queue.mu.Unlock()
		update, err = stream.Next(t.Context())
		if err != nil {
			t.Fatal(err)
		}
		value = update.Data.(protocol.UpdateInstallationsPayload)
		if len(value.Tasks) != 1 || value.Tasks[0].Request != second {
			t.Fatal("a later installation with another scope entered the captured stream")
		}
		pending, err := queue.SubscribeFor(node, []protocol.RequestID{second}, captured)
		if err != nil {
			t.Fatal(err)
		}
		defer pending.Close()
		close(serviceClosed)
		_, err = pending.Next(t.Context())
		assertFault(err, protocol.ErrorCodeUnavailable)
	})
	owner := tasks.New(t.Context())
	defer owner.Close(context.Background())
	node := protocol.NodeID{1}
	queue := New(owner, node, t.Context().Done())
	first := protocol.NewRequest(node, protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: "one"}})
	second, other := first, first
	second.ID, other.ID = protocol.NewRequestID(), protocol.NewRequestID()
	resource := protocol.InstallationResource{Kind: "plugin", Data: protocol.InstallationResourcePluginPayload{Name: "one"}}
	started, release, secondStarted, secondRelease, otherStarted := make(chan struct{}), make(chan struct{}), make(chan struct{}), make(chan struct{}), make(chan struct{})
	firstDone, secondDone, otherDone := make(chan struct{}), make(chan struct{}), make(chan struct{})
	if err := queue.Submit(node, first, resource, true, func(ctx context.Context, report Report) link.Response {
		close(started)
		report(protocol.InstallationPhaseDownloading, &protocol.InstallationProgress{Kind: "percent", Data: protocol.InstallationProgressPercentPayload{Percent: 25}})
		<-release
		defer close(firstDone)
		return link.Response{}
	}); err != nil {
		t.Fatal(err)
	}
	<-started
	if err := queue.Submit(node, second, resource, true, func(ctx context.Context, report Report) link.Response {
		close(secondStarted)
		<-secondRelease
		defer close(secondDone)
		return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture cleanup is uncertain"}}
	}); err != nil {
		t.Fatal(err)
	}
	resource.Data = protocol.InstallationResourcePluginPayload{Name: "two"}
	if err := queue.Submit(node, other, resource, true, func(ctx context.Context, report Report) link.Response {
		close(otherStarted)
		defer close(otherDone)
		return link.Response{}
	}); err != nil {
		t.Fatal(err)
	}
	<-otherStarted
	select {
	case <-secondStarted:
		t.Fatal("a package overtook its predecessor's durable completion")
	default:
	}
	if err := queue.Dismiss(node, first.ID, nil); err == nil {
		t.Fatal("active installation was dismissed")
	}
	close(release)
	<-firstDone
	<-secondStarted
	close(secondRelease)
	<-secondDone
	<-otherDone
	if err := owner.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	records, err := queue.Read(node, []protocol.RequestID{first.ID, second.ID, other.ID}, nil)
	if err != nil || len(records) != 1 || records[0].Request != second.ID || records[0].State.Kind != "uncertain" {
		t.Fatalf("retained installations=%v error=%v", records, err)
	}
	if err := queue.Dismiss(protocol.NodeID{2}, second.ID, nil); err != nil {
		t.Fatal(err)
	}
	records, _ = queue.Read(node, []protocol.RequestID{second.ID}, nil)
	if len(records) != 1 {
		t.Fatal("another caller dismissed an installation")
	}
	if err := queue.Dismiss(node, second.ID, nil); err != nil {
		t.Fatal(err)
	}
	records, _ = queue.Read(node, []protocol.RequestID{second.ID}, nil)
	if len(records) != 0 {
		t.Fatal("dismissal retained a terminal installation")
	}
}
