package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationNodeIDUsesCanonicalHex(t *testing.T) {
	id := protocol.NodeID{1, 2, 3}
	if value, ok := configurationNodeID(id.String()); !ok || value != id {
		t.Fatal("canonical identity was not preserved")
	}
	for _, raw := range []string{"", "abc", "not-an-id"} {
		if _, ok := configurationNodeID(raw); ok {
			t.Fatal("invalid identity was accepted")
		}
	}
}

func TestConfigurationFollowProjectionKeepsControllerAndHostStatus(t *testing.T) {
	source, target := protocol.NodeID{8}, protocol.NodeID{9}
	r := &Runtime{App: &shell.App{Settings: settings.NewLive()}}
	r.App.Settings.Devices = []viewmodel.Device{{ID: source.String(), Name: "Controller"}, {ID: target.String(), Name: "Execution host"}}
	view := &settings.ConfigurationFollowEditor{}
	status := protocol.ConfigurationFollowStatus{
		User:   &protocol.User{Administrator: true, Controller: &source},
		Source: &target,
		Targets: []protocol.ConfigurationFollowTarget{{Host: target, Error: &protocol.Fault{
			Code: protocol.ErrorCodeUnavailable, Message: "Host is offline",
		}}},
	}
	r.projectConfigurationFollow(view, status)
	if view.Local || view.Source != "Controller · "+source.String()[:8] || len(view.Targets) != 1 || view.Targets[0].Name != "Execution host · "+target.String()[:8] || view.Targets[0].Failure != "Host is offline" {
		t.Fatalf("controller namespace or host error was lost: %+v", view)
	}
	r.projectConfigurationFollow(view, protocol.ConfigurationFollowStatus{User: &protocol.User{Local: true}})
	if !view.Local || view.Source != "" || len(view.Targets) != 0 {
		t.Fatalf("a previous host's status leaked into local configuration: %+v", view)
	}
}

func TestConfigurationFollowStatusAndRetryUseSelectedHost(t *testing.T) {
	localSnapshot, remoteSnapshot := protocol.DefaultSnapshot(), protocol.DefaultSnapshot()
	localSnapshot.Node, remoteSnapshot.Node = protocol.NodeID{1}, protocol.NodeID{2}
	localTransport := &conversationTestTransport{snapshot: localSnapshot}
	remoteTransport := &conversationTestTransport{snapshot: remoteSnapshot}
	uncertain := true
	remoteTransport.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind == "inspect_request" {
			query, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: protocol.RequestOutcome{Kind: "admitted"}}}}, true
		}
		if request.Command.Kind != "configuration_follow" {
			return link.Response{}, false
		}
		command, _ := detail[protocol.ConfigurationFollowCommand](request.Command.Data)
		if command.Action == "sync" && uncertain {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "Sync outcome was not observed"}}, true
		}
		return link.Response{Output: protocol.Output{Kind: "configuration_follow", Data: protocol.ConfigurationFollowStatus{Source: &localSnapshot.Node}}}, true
	}
	localClient, remoteClient := client.New(localTransport), client.New(remoteTransport)
	defer localClient.Close()
	defer remoteClient.Close()
	if err := remoteClient.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	app := &shell.App{Settings: settings.NewLive()}
	root := &Runtime{ctx: t.Context(), controller: localClient, App: app}
	remote := &Runtime{ctx: t.Context(), controller: remoteClient, App: app, parent: root, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
	root.bindings = map[string]*Runtime{remoteSnapshot.Node.String(): remote}
	root.initConfigurationFollowActions()
	tester := ui.NewTester(func(c *ui.Context) {
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, post := range queue {
			post(c)
		}
		kit.ToastHost(c)
	}, 640, 480)
	settle := func() {
		for range 4 {
			root.workers.Wait()
			remote.workers.Wait()
			tester.Frame()
		}
	}
	defer settle()
	app.Settings.FollowActions.Open(viewmodel.Device{ID: remoteSnapshot.Node.String()})
	settle()
	view := app.Settings.FollowEditor
	if view == nil || view.Loading || view.ReadFailed || view.Source != localSnapshot.Node.String()[:12] {
		t.Fatalf("selected Host status was not read: %+v", view)
	}
	app.Settings.FollowActions.Refresh(view)
	settle()
	if view.Pending || len(remote.uncertain) != 1 {
		t.Fatalf("unconfirmed sync did not retain its request: pending=%v, requests=%d", view.Pending, len(remote.uncertain))
	}
	remoteTransport.mu.Lock()
	uncertain = false
	remoteTransport.mu.Unlock()
	app.Settings.FollowActions.Refresh(view)
	settle()
	remoteTransport.mu.Lock()
	defer remoteTransport.mu.Unlock()
	var requests []protocol.Request
	for _, request := range remoteTransport.requests {
		if request.Target != remoteSnapshot.Node {
			t.Fatalf("status or retry was sent to another Host: %+v", request)
		}
		if request.Command.Kind == "inspect_request" {
			continue
		}
		command, ok := detail[protocol.ConfigurationFollowCommand](request.Command.Data)
		if request.Command.Kind != "configuration_follow" || !ok || command.Action != "read" && command.Action != "sync" {
			t.Fatalf("status read sent an unrelated command: %+v", request.Command)
		}
		if command.Action == "sync" {
			requests = append(requests, request)
		}
	}
	if len(requests) != 2 || requests[0].ID != requests[1].ID || len(remote.uncertain) != 0 {
		t.Fatalf("retry did not settle the original sync request: %+v", requests)
	}
	localTransport.mu.Lock()
	defer localTransport.mu.Unlock()
	if len(localTransport.requests) != 0 {
		t.Fatal("selected Host status or retry was sent to the local Node")
	}
}
