package installations

import (
	"context"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/tasks"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestInstallQueueOrder(t *testing.T) {
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
