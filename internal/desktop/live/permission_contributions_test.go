package live

import (
	"context"
	"errors"
	"slices"
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/plugins/permissionflow"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	computer "github.com/sailry/sailry-harness/modules/computer/desktop"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

type permissionCommandTrace struct {
	link.Transport
	mu       sync.Mutex
	commands []protocol.Command
	topics   []string
}

func (trace *permissionCommandTrace) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	switch request.Command.Kind {
	case "read_computer_settings", "read_computer_permissions", "request_computer_permission":
		trace.mu.Lock()
		trace.commands = append(trace.commands, request.Command)
		trace.mu.Unlock()
	}
	return trace.Transport.Dispatch(ctx, request)
}

func (trace *permissionCommandTrace) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	trace.mu.Lock()
	trace.topics = append(trace.topics, topic.Kind)
	trace.mu.Unlock()
	return trace.Transport.Subscribe(ctx, topic)
}

func (trace *permissionCommandTrace) sent(kind string) []protocol.Command {
	trace.mu.Lock()
	defer trace.mu.Unlock()
	var commands []protocol.Command
	for _, command := range trace.commands {
		if kind == "" || kind == command.Kind {
			commands = append(commands, command)
		}
	}
	return commands
}

func (trace *permissionCommandTrace) subscriptions() []string {
	trace.mu.Lock()
	defer trace.mu.Unlock()
	return slices.Clone(trace.topics)
}

type permissionFixtureLink struct {
	link.Transport
	snapshot protocol.Snapshot
	handle   func(protocol.Request) (link.Response, bool)
}

func (source *permissionFixtureLink) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	if source.handle != nil {
		if answer, ok := source.handle(request); ok {
			completion := make(chan link.Response, 1)
			completion <- answer
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}, nil
		}
	}
	return link.Admission{}, errors.New("unexpected permission fixture command")
}

func (source *permissionFixtureLink) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "node" {
		return nil, errors.New("unexpected permission fixture subscription")
	}
	return &permissionFixtureSubscription{snapshot: &source.snapshot, closed: make(chan struct{})}, nil
}

type permissionFixtureSubscription struct {
	snapshot *protocol.Snapshot
	closed   chan struct{}
	once     sync.Once
}

func (stream *permissionFixtureSubscription) Next(ctx context.Context) (protocol.Update, error) {
	if snapshot := stream.snapshot; snapshot != nil {
		stream.snapshot = nil
		return protocol.Update{Kind: "snapshot", Data: *snapshot}, nil
	}
	select {
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-stream.closed:
		return protocol.Update{}, context.Canceled
	}
}

func (stream *permissionFixtureSubscription) Close() error {
	stream.once.Do(func() { close(stream.closed) })
	return nil
}

func permissionTestController(t *testing.T, node protocol.NodeID, handle func(protocol.Request) (link.Response, bool)) (*client.Client, *permissionCommandTrace) {
	t.Helper()
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.User = node, &protocol.User{Administrator: true}
	trace := &permissionCommandTrace{Transport: &permissionFixtureLink{
		Transport: link.NewLocal(node, node, nil), snapshot: snapshot, handle: handle,
	}}
	controller := client.New(trace)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	synctest.Wait()
	if !controller.Connected() || controller.Snapshot().User == nil {
		t.Fatal("permission fixture has no connected Host")
	}
	return controller, trace
}

func permissionTestRuntime(t *testing.T, controller *client.Client, assembly native.Assembly) *Runtime {
	t.Helper()
	ctx, cancel := context.WithCancel(t.Context())
	r := &Runtime{controller: controller, ctx: ctx, cancel: cancel, App: shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)}
	r.permissions.contributions = permissionContributions(assembly.Permissions)
	r.permissions.probe = permissionProbe{goos: "darwin", check: func(permissionflow.Permission) (permissionflow.Status, error) { return permissionflow.Granted, nil }}
	t.Cleanup(func() { cancel(); r.workers.Wait() })
	r.App.Navigate(shell.SettingsPage)
	return r
}

func settlePermissionWork(r *Runtime) {
	for range 3 {
		synctest.Wait()
		r.mu.Lock()
		queued := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, apply := range queued {
			apply(nil)
		}
		r.permissionsFrame()
	}
	synctest.Wait()
}

func TestPermissionsFollowComputerAssembly(t *testing.T) {
	for _, installed := range []bool{false, true} {
		name := "absent"
		if installed {
			name = "installed"
		}
		t.Run(name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				var screen atomic.Bool
				grant := true
				handle := func(request protocol.Request) (link.Response, bool) {
					switch request.Command.Kind {
					case "read_computer_settings":
						return link.Response{Output: protocol.Output{Kind: "computer_settings", Data: protocol.ComputerSettings{Enabled: true}}}, true
					case "request_computer_permission":
						screen.Store(true)
						fallthrough
					case "read_computer_permissions":
						allowed := screen.Load()
						value := protocol.ComputerPermissions{Platform: "macos", Local: true, Accessibility: &grant, ScreenCapture: &allowed}
						return link.Response{Output: protocol.Output{Kind: "computer_permissions", Data: value}}, true
					}
					return link.Response{}, false
				}
				controller, trace := permissionTestController(t, protocol.NodeID{1}, handle)
				var assembly native.Assembly
				if installed {
					assembly = native.Assemble(computer.Register)
				}
				r := permissionTestRuntime(t, controller, assembly)
				r.App.Settings.Open(settings.General)
				r.recoverFailedReads(true)
				settlePermissionWork(r)
				if sent := trace.sent(""); len(sent) != 0 {
					t.Fatalf("another settings page read helper permissions: %v", sent)
				}
				r.App.Settings.Open(settings.Permissions)
				settlePermissionWork(r)
				actions := r.App.Settings.PermissionActions
				if !installed {
					actions.Check()
					actions.HostRequest("permission_screen")
					settlePermissionWork(r)
					if topics := trace.subscriptions(); len(actions.HostPermissions) != 0 || len(trace.sent("")) != 0 || !slices.Equal(topics, []string{"node"}) {
						t.Fatalf("an absent module supplied rows, commands, or subscriptions: %v %v %v", actions.HostPermissions, trace.sent(""), topics)
					}
					return
				}
				if reads, checks := trace.sent("read_computer_settings"), trace.sent("read_computer_permissions"); len(reads) != 1 || len(checks) != 1 {
					t.Fatalf("the same subject did not share its enabled read and grant check: %v %v", reads, checks)
				}
				if rows := actions.HostPermissions; len(rows) != 2 || rows[0].Status != "permission_granted" || rows[1].Status != "permission_required" {
					t.Fatalf("the helper's two grants were not applied: %+v", rows)
				}
				screen.Store(true) // The UI's last check is stale.
				actions.HostRequest("permission_screen")
				settlePermissionWork(r)
				if len(trace.sent("request_computer_permission")) != 0 || actions.HostPermissions[1].Status != "permission_granted" {
					t.Fatal("a grant already made was requested again")
				}
				screen.Store(false)
				actions.Check()
				settlePermissionWork(r)
				actions.HostRequest("permission_screen")
				settlePermissionWork(r)
				sent := trace.sent("request_computer_permission")
				if len(sent) != 1 || actions.HostPermissions[1].Status != "permission_granted" {
					t.Fatalf("the missing local grant was not requested once: %v %v", sent, actions.HostPermissions)
				}
				payload, ok := detail[protocol.CommandRequestComputerPermissionPayload](sent[0].Data)
				if !ok || payload.Permission != protocol.ComputerPermissionScreenCapture {
					t.Fatalf("screen recording mapped to %v", sent[0].Data)
				}

				screen.Store(false)
				peer, remoteTrace := permissionTestController(t, protocol.NodeID{2}, handle)
				r.bindings = map[string]*Runtime{peer.Target().String(): {parent: r, controller: peer, ctx: r.ctx}}
				r.App.Settings.ManagedNode = peer.Target().String()
				settlePermissionWork(r)
				actions.HostRequest("permission_screen")
				settlePermissionWork(r)
				if !actions.HostReadOnly || len(remoteTrace.sent("read_computer_permissions")) != 1 || len(remoteTrace.sent("request_computer_permission")) != 0 {
					t.Fatalf("the remote helper was not read-only: %v", remoteTrace.sent(""))
				}
			})
		})
	}
}

func TestPermissionContributionReadScope(t *testing.T) {
	for _, stage := range []string{"availability", "grants"} {
		t.Run(stage, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				started := make(chan context.Context, 1)
				release := make(chan struct{})
				var held atomic.Bool
				wait := func(ctx context.Context) bool {
					if held.CompareAndSwap(false, true) {
						started <- ctx
						<-release // A late response is possible even after cancellation.
						return true
					}
					return false
				}
				var once sync.Once
				finish := func() { once.Do(func() { close(release) }) }
				assembly := native.Assemble(func(a *native.Assembly) {
					a.Permissions = []native.PermissionContribution{{
						Subject: "fixture.helper", Kinds: []native.PermissionKind{native.PermissionKindScreenRecording},
						Available: func(ctx context.Context, _ native.PermissionCommand) (bool, error) {
							return stage != "availability" || wait(ctx), nil
						},
						Check: func(ctx context.Context, _ native.PermissionCommand) (native.PermissionReport, error) {
							state := native.PermissionRequired
							if stage == "grants" && wait(ctx) {
								state = native.PermissionGranted
							}
							return native.PermissionReport{Grants: map[native.PermissionKind]native.PermissionGrantState{native.PermissionKindScreenRecording: state}}, nil
						},
					}}
				})
				controller, _ := permissionTestController(t, protocol.NodeID{1}, nil)
				r := permissionTestRuntime(t, controller, assembly)
				defer finish()
				r.App.Settings.Open(settings.Permissions)
				settlePermissionWork(r)
				var old context.Context
				select {
				case old = <-started:
				default:
					t.Fatal("the old scope did not start the read")
				}
				if stage == "availability" {
					r.App.Settings.Open(settings.General)
					settlePermissionWork(r)
					if r.permissions.scope != nil || r.App.Settings.PermissionActions != nil {
						t.Fatal("leaving Permissions retained its bindings")
					}
				} else {
					r.App.Settings.ManagedNode = protocol.NodeID{2}.String()
					settlePermissionWork(r)
				}
				if !errors.Is(old.Err(), context.Canceled) {
					t.Fatalf("the retired %s read was not cancelled: %v", stage, old.Err())
				}
				r.App.Settings.ManagedNode = ""
				r.App.Settings.Open(settings.Permissions)
				settlePermissionWork(r)
				finish()
				settlePermissionWork(r)
				rows := r.App.Settings.PermissionActions.HostPermissions
				if stage == "availability" && len(rows) != 0 || stage == "grants" && (len(rows) != 1 || rows[0].Status != "permission_required") {
					t.Fatalf("a late %s result entered the reopened scope: %v", stage, rows)
				}
			})
		})
	}
}

func TestPermissionContributionRecovery(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var attempts atomic.Int32
		assembly := native.Assemble(func(a *native.Assembly) {
			a.Permissions = []native.PermissionContribution{{
				Subject: "fixture.helper", Kinds: []native.PermissionKind{native.PermissionKindScreenRecording},
				Available: func(context.Context, native.PermissionCommand) (bool, error) {
					if attempts.Add(1) < 3 {
						return false, errors.New("temporary fixture failure")
					}
					return true, nil
				},
				Check: func(context.Context, native.PermissionCommand) (native.PermissionReport, error) {
					return native.PermissionReport{Grants: map[native.PermissionKind]native.PermissionGrantState{
						native.PermissionKindScreenRecording: native.PermissionGranted,
					}}, nil
				},
			}}
		})
		controller, _ := permissionTestController(t, protocol.NodeID{1}, nil)
		r := permissionTestRuntime(t, controller, assembly)
		r.App.Settings.Open(settings.Permissions)
		settlePermissionWork(r)
		settlePermissionWork(r)
		if attempts.Load() != 1 {
			t.Fatalf("drawing retried a failed availability read %d times", attempts.Load())
		}
		r.recoverFailedReads(false)
		settlePermissionWork(r)
		if attempts.Load() != 2 {
			t.Fatalf("reconnect did not release the failed read: %d attempts", attempts.Load())
		}
		r.App.Settings.PermissionActions.Check()
		settlePermissionWork(r)
		rows := r.App.Settings.PermissionActions.HostPermissions
		if attempts.Load() != 3 || len(rows) != 1 || rows[0].Status != "permission_granted" {
			t.Fatalf("Check did not recover the source: attempts %d, rows %v", attempts.Load(), rows)
		}
		r.App.Settings.Open(settings.General)
		r.recoverFailedReads(true)
		settlePermissionWork(r)
		if attempts.Load() != 3 || r.permissions.scope != nil {
			t.Fatal("refresh outside the page reactivated a permission source")
		}
	})
}
