package live

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type pluginLocationTestNode struct {
	pluginViewTestNode
	prepared map[protocol.RequestID]protocol.Request
	outcome  protocol.RequestOutcome
	commands []string
	pending  chan link.Response
	dispatch func(protocol.Request) (link.Admission, error)
}

func (source *pluginLocationTestNode) Dispatch(_ context.Context, _ protocol.NodeID, request protocol.Request) (link.Admission, error) {
	source.commands = append(source.commands, request.Command.Kind)
	if source.dispatch != nil && request.Command.Kind != "inspect_request" {
		return source.dispatch(request)
	}
	query, valid := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
	if request.Command.Kind != "inspect_request" || !valid {
		return link.Admission{}, errors.New("fixture accepts only inspection of captured requests")
	}
	prepared, exists := source.prepared[query.ID]
	if !exists {
		return link.Admission{}, errors.New("fixture request is unavailable")
	}
	body, err := json.Marshal(prepared)
	digest := blake3.Sum256(body)
	if err != nil || query.Digest != hex.EncodeToString(digest[:]) {
		return link.Admission{}, errors.New("fixture request identity changed before inspection")
	}
	completion := source.pending
	if completion == nil {
		completion = make(chan link.Response, 1)
		completion <- link.Response{Output: protocol.Output{
			Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: source.outcome},
		}}
	}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

type pluginLocationTestFixture struct {
	root   *Runtime
	state  *pluginNativeState
	source *pluginLocationTestNode
	host   *desktopplugins.Host
	main   protocol.Worktree
	target protocol.Worktree
}

func newPluginLocationTestFixture(t *testing.T) pluginLocationTestFixture {
	t.Helper()
	// MyGo's raster helpers are process-wide goroutines. A frame painted inside
	// a synctest bubble must stay on the bubble's own goroutine.
	processors := runtime.GOMAXPROCS(1)
	t.Cleanup(func() { runtime.GOMAXPROCS(processors) })
	project := protocol.DefaultProject()
	project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Captured project", "/fixture"
	main := protocol.Worktree{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: project.Path, Main: true}
	target := protocol.Worktree{ID: protocol.WorktreeID{2}, Project: &project.ID, Path: "/fixture/created"}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Projects, snapshot.Worktrees = protocol.NodeID{1}, []protocol.Project{project}, []protocol.Worktree{main}
	source := &pluginLocationTestNode{
		pluginViewTestNode: pluginViewTestNode{snapshot: snapshot},
		prepared:           map[protocol.RequestID]protocol.Request{},
	}
	ctx, cancel := context.WithCancel(t.Context())
	controller := client.New(link.NewLocal(snapshot.Node, snapshot.Node, source))
	if err := controller.Start(ctx); err != nil {
		cancel()
		t.Fatal(err)
	}
	data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}}
	for _, id := range []string{"draft/first", "draft/second"} {
		data.Unassigned = append(data.Unassigned, &viewmodel.Conversation{ID: id, Node: snapshot.Node.String()})
	}
	root := &Runtime{
		ctx: ctx, cancel: cancel, controller: controller, packages: newPackageState(),
		App: shell.NewLive(data), Preferences: preferences.Open(filepath.Join(t.TempDir(), "preferences.json")),
	}
	root.initPluginViews()
	root.initWorkspace()
	for _, draft := range data.Unassigned {
		comp := root.App.Composer(draft.ID)
		comp.Host, comp.Project, comp.Worktree = snapshot.Node.String(), project.ID.String(), main.Path
	}
	root.App.Sidebar.Selected = "draft/second"
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "location-fixture", strings.Repeat("a", 64), true
	info.Extension = new(protocol.DefaultPluginExtension())
	info.Extension.Actions = []protocol.PluginAction{
		protocol.PluginActionReadWorktrees, protocol.PluginActionWriteWorktrees, protocol.PluginActionControlConversation,
	}
	root.packages.infos[info.Summary.Name] = info
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Worktree, scope.Surface.Kind = packageReference(info.Summary), &main.ID, "composer"
	native := desktopplugins.NewNativeBinding(ctx)
	native.Operations = map[string]desktopplugins.HostOperation{}
	state := &pluginNativeState{
		runtime: root, scope: scope, presentation: "draft/first", native: &native,
		composer: &pluginNativeComposer{source: "draft/first"},
	}
	state.locationOperations()
	host := desktopplugins.NewHost(desktopplugins.HostBinding{
		Context: ctx, Client: controller, Scope: scope, Native: &native, Actions: info.Extension.Actions,
	})
	t.Cleanup(func() {
		_ = host.Close()
		root.closePluginViews()
		root.closeWorkspace()
		cancel()
		root.workers.Wait()
		root.observers.Wait()
		_ = controller.Close()
	})
	return pluginLocationTestFixture{root: root, state: state, source: source, host: host, main: main, target: target}
}

func (fixture pluginLocationTestFixture) prepare(t *testing.T, command protocol.Command) protocol.Request {
	t.Helper()
	modules, err := fixture.host.Modules()
	if err != nil {
		t.Fatal(err)
	}
	for _, module := range modules {
		if module.Name != "sailry/sdk" {
			continue
		}
		for _, export := range module.Exports {
			if export.Name != "prepareRequest" {
				continue
			}
			body, err := json.Marshal(command)
			if err != nil {
				t.Fatal(err)
			}
			value, err := export.Sync(fixture.root.ctx, []json.RawMessage{body})
			var id protocol.RequestID
			if err != nil || json.Unmarshal(value, &id) != nil {
				t.Fatalf("fixture request was not prepared by its real Host: %s, %v", value, err)
			}
			request, err := fixture.state.native.PreparedRequest(fixture.root.ctx, id.String())
			if err != nil {
				t.Fatal(err)
			}
			fixture.source.prepared[id] = request
			return request
		}
	}
	t.Fatal("fixture Host did not expose request preparation")
	return protocol.Request{}
}

func pluginLocationTestArguments(t *testing.T, values ...any) []json.RawMessage {
	t.Helper()
	arguments := make([]json.RawMessage, 0, len(values))
	for _, value := range values {
		body, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		arguments = append(arguments, body)
	}
	return arguments
}

func pluginLocationTestFrame(root *Runtime) func() {
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queue {
			apply(view)
		}
	}, 800, 600)
	return func() { tester.Frame() }
}

func (fixture pluginLocationTestFixture) call(t *testing.T, operation string, arguments []json.RawMessage) (json.RawMessage, error) {
	t.Helper()
	type result struct {
		body json.RawMessage
		err  error
	}
	completed := make(chan result, 1)
	go func() {
		body, err := fixture.state.native.Operations[operation](fixture.root.ctx, arguments)
		completed <- result{body, err}
	}()
	frame := pluginLocationTestFrame(fixture.root)
	for range 2 {
		synctest.Wait()
		frame()
	}
	synctest.Wait()
	select {
	case value := <-completed:
		return value.body, value.err
	default:
		t.Fatal("location operation did not settle after its queued confirmation and application")
		return nil, nil
	}
}

func pluginLocationTestOutcome(output protocol.Output) protocol.RequestOutcome {
	return protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}}
}

func testPluginLocationHandoffs(t *testing.T) {
	t.Run("a source change while inspection is pending prevents handoff", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			fixture.source.pending = make(chan link.Response, 1)
			request := fixture.prepare(t, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{
				Project: *fixture.main.Project, Path: fixture.target.Path,
			}})
			completed := make(chan error, 1)
			go func() {
				_, err := fixture.state.native.Operations["sailry/ui.worktreeHasDrafts"](
					fixture.root.ctx, pluginLocationTestArguments(t, fixture.target.ID, request.ID),
				)
				completed <- err
			}()
			frame := pluginLocationTestFrame(fixture.root)
			synctest.Wait()
			frame()
			synctest.Wait()
			fixture.root.App.Composer("draft/first").Worktree = "/fixture/other"
			fixture.source.pending <- link.Response{Output: protocol.Output{
				Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{
					ID: request.ID, Outcome: pluginLocationTestOutcome(protocol.Output{Kind: "worktree", Data: fixture.target}),
				},
			}}
			synctest.Wait()
			frame()
			synctest.Wait()
			select {
			case err := <-completed:
				if err == nil || len(fixture.root.workspace.admitted) != 0 || len(fixture.source.commands) != 1 {
					t.Fatalf("late handoff ignored its changed captured source: %v", err)
				}
			default:
				t.Fatal("late handoff did not settle after its captured source changed")
			}
		})
	})
	t.Run("selection confirms its original request and reports the created location", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			request := fixture.prepare(t, protocol.Command{Kind: "create_managed_worktree", Data: protocol.CommandCreateManagedWorktreePayload{
				Project: *fixture.main.Project, Source: fixture.main.ID, Branch: "created",
			}})
			fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree", Data: fixture.target})
			body, err := fixture.call(t, "sailry/ui.selectLocation", pluginLocationTestArguments(t, "1", map[string]any{
				"worktree": fixture.target.ID, "request": request.ID,
			}))
			if err != nil || string(body) != "true" {
				t.Fatalf("confirmed selection was not applied: %s, %v", body, err)
			}
			scope := composerPluginScope(fixture.root, fixture.root.App.Data.Find("draft/first"), fixture.root.App.Composer("draft/first"))
			if scope.Worktree == nil || *scope.Worktree != fixture.target.ID {
				t.Fatal("new contribution scope ignored the confirmed worktree before snapshot admission")
			}
			fixture.state.native.Frame(nil)
			body, err = fixture.state.native.Operations["sailry/ui.nextLocationChange"](fixture.root.ctx, pluginLocationTestArguments(t, "1"))
			var changed struct {
				Cursor   string              `json:"cursor"`
				Worktree protocol.WorktreeID `json:"worktree"`
				Main     bool                `json:"main"`
			}
			if err != nil || json.Unmarshal(body, &changed) != nil || changed.Cursor != "2" ||
				changed.Worktree != fixture.target.ID || changed.Main {
				t.Fatalf("confirmed worktree metadata was lost before snapshot admission: %s, %v", body, err)
			}
			if _, err := fixture.state.native.Operations["sailry/ui.readLocation"](fixture.root.ctx, nil); err == nil {
				t.Fatal("the original worktree scope read its retargeted source")
			}
		})
	})
	for _, kind := range []string{"register_worktree", "create_worktree", "create_managed_worktree"} {
		t.Run(kind+" retains its exact confirmed output before snapshot admission", func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t)
				var data any
				switch kind {
				case "register_worktree":
					data = protocol.CommandRegisterWorktreePayload{Project: *fixture.main.Project, Path: fixture.target.Path}
				case "create_worktree":
					data = protocol.CommandCreateWorktreePayload{Project: *fixture.main.Project, Path: fixture.target.Path, Branch: "created", Commit: "fixture"}
				case "create_managed_worktree":
					data = protocol.CommandCreateManagedWorktreePayload{Project: *fixture.main.Project, Source: fixture.main.ID, Branch: "created"}
				}
				request := fixture.prepare(t, protocol.Command{Kind: kind, Data: data})
				fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree", Data: fixture.target})
				body, err := fixture.call(t, "sailry/ui.worktreeHasDrafts", pluginLocationTestArguments(t, fixture.target.ID, request.ID))
				if err != nil || string(body) != "false" || fixture.root.workspace.admitted[fixture.target.ID].Path != fixture.target.Path ||
					len(fixture.source.commands) != 1 || fixture.source.commands[0] != "inspect_request" {
					t.Fatalf("captured handoff was not inspected once and retained: %s, %v, %v", body, err, fixture.source.commands)
				}
				body, err = fixture.call(t, "sailry/ui.selectLocation", pluginLocationTestArguments(t, "1", map[string]any{"worktree": fixture.target.ID}))
				if err != nil || string(body) != "true" || fixture.root.App.Composer("draft/first").Worktree != fixture.target.Path ||
					fixture.root.App.Composer("draft/second").Worktree != fixture.main.Path || len(fixture.source.commands) != 1 {
					t.Fatalf("confirmed selection changed its source or replayed a command: %s, %v", body, err)
				}
			})
		})
	}
	for _, name := range []string{"wrong project", "wrong managed source", "wrong command", "missing target", "stale cursor", "unknown prepared request"} {
		t.Run(name+" cannot inspect or admit a handoff", func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t)
				command := protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{
					Project: *fixture.main.Project, Path: fixture.target.Path,
				}}
				switch name {
				case "wrong project":
					command.Data = protocol.CommandRegisterWorktreePayload{Project: protocol.ProjectID{9}, Path: fixture.target.Path}
				case "wrong managed source":
					command = protocol.Command{Kind: "create_managed_worktree", Data: protocol.CommandCreateManagedWorktreePayload{
						Project: *fixture.main.Project, Source: protocol.WorktreeID{9},
					}}
				case "wrong command":
					command = protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: fixture.target.ID}}
				}
				request := fixture.prepare(t, command)
				fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree", Data: fixture.target})
				selection := map[string]any{"worktree": fixture.target.ID, "request": request.ID}
				cursor := "1"
				switch name {
				case "missing target":
					selection = map[string]any{"projects": true, "request": request.ID}
				case "stale cursor":
					cursor = "0"
				case "unknown prepared request":
					selection["request"] = protocol.RequestID{9}
				}
				_, err := fixture.call(t, "sailry/ui.selectLocation", pluginLocationTestArguments(t, cursor, selection))
				if err == nil || len(fixture.source.commands) != 0 || len(fixture.root.workspace.admitted) != 0 {
					t.Fatalf("invalid handoff inspected or admitted a worktree: %v, %v", err, fixture.source.commands)
				}
			})
		})
	}
	for _, name := range []string{"not admitted", "admitted", "unknown", "fault", "wrong output", "wrong worktree", "wrong project"} {
		t.Run(name+" does not confirm a handoff", func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t)
				request := fixture.prepare(t, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{
					Project: *fixture.main.Project, Path: fixture.target.Path,
				}})
				tree := fixture.target
				switch name {
				case "not admitted":
					fixture.source.outcome.Kind = "not_admitted"
				case "admitted", "unknown":
					fixture.source.outcome.Kind = name
				case "fault":
					failure := protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "fixture conflict"}
					fixture.source.outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Err: &failure}}
				case "wrong output":
					fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree_removed", Data: protocol.OutputWorktreeRemovedPayload{ID: tree.ID}})
				case "wrong worktree":
					tree.ID = protocol.WorktreeID{9}
					fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree", Data: tree})
				case "wrong project":
					tree.Project = new(protocol.ProjectID{9})
					fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree", Data: tree})
				}
				_, err := fixture.call(t, "sailry/ui.worktreeHasDrafts", pluginLocationTestArguments(t, fixture.target.ID, request.ID))
				if faultCode(err) != protocol.ErrorCodeUnavailable ||
					len(fixture.source.commands) != 1 || len(fixture.root.workspace.admitted) != 0 {
					t.Fatalf("unconfirmed handoff admitted a worktree or replayed its command: %v, %v", err, fixture.source.commands)
				}
			})
		})
	}
}

func testPluginSessionBase(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newPluginLocationTestFixture(t)
		root := fixture.root
		root.sessionBases, root.conversationCreating = map[*composer.State]*sessionBaseStart{}, map[string]bool{}
		root.conversationCreations, root.busy, root.uncertain = map[string]conversationCreation{}, map[string]bool{}, map[string]protocol.Request{}
		comp, view := root.App.Composer("draft/first"), root.App.Data.Find("draft/first")
		base := map[string]any{"from": "main", "branch": "feature", "commit": strings.Repeat("a", 40)}
		if _, err := fixture.call(t, "sailry/ui.selectLocation", pluginLocationTestArguments(t, "1", map[string]any{"base": base})); err == nil {
			t.Fatal("a directory without a repository accepted a session base")
		}
		project := root.workspaceFor(fixture.main)
		project.statusKnown, project.status.Kind = true, protocol.RepositoryKindReady
		cursor := func() string {
			body, err := fixture.state.native.Operations["sailry/ui.readLocation"](root.ctx, nil)
			var value struct {
				Cursor string `json:"cursor"`
			}
			if err != nil || json.Unmarshal(body, &value) != nil {
				t.Fatalf("location is unavailable: %s, %v", body, err)
			}
			return value.Cursor
		}
		for _, invalid := range []map[string]any{
			{"from": "main", "branch": "feature", "commit": "abc"},
			{"from": "", "branch": "feature", "commit": base["commit"]},
			{"from": "main", "branch": "feature", "commit": base["commit"], "extra": true},
		} {
			if _, err := fixture.call(t, "sailry/ui.selectLocation", pluginLocationTestArguments(t, cursor(), map[string]any{"base": invalid})); err == nil {
				t.Fatalf("invalid session base was accepted: %v", invalid)
			}
		}
		body, err := fixture.call(t, "sailry/ui.selectLocation", pluginLocationTestArguments(t, cursor(), map[string]any{"base": base}))
		if err != nil || string(body) != "true" || len(fixture.source.commands) != 0 {
			t.Fatalf("session base was not retained without a command: %s, %v", body, err)
		}
		body, err = fixture.state.native.Operations["sailry/ui.readLocation"](root.ctx, nil)
		var read struct {
			Base *sessionBase `json:"base"`
		}
		if err != nil || json.Unmarshal(body, &read) != nil || read.Base == nil || read.Base.Branch != "feature" {
			t.Fatalf("location did not report its pending base: %s, %v", body, err)
		}
		var requests []protocol.Request
		fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
			requests = append(requests, request)
			if len(requests) == 1 {
				return link.Admission{}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "completion was lost"}}
			}
			completion := make(chan link.Response, 1)
			completion <- link.Response{Output: protocol.Output{Kind: "worktree", Data: fixture.target}}
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
		}
		frame := pluginLocationTestFrame(root)
		settled := 0
		start := func() {
			(&conversationActions{r: root, cv: view, comp: comp}).startSessionBase(root.pendingSessionBase(comp), func(_ *ui.Context, err error) {
				settled++
				if (err == nil) != (settled == 2) {
					t.Errorf("attempt %d settled with %v", settled, err)
				}
			})
			if !root.conversationCreating[view.ID] {
				t.Error("base creation did not hold the draft busy")
			}
			synctest.Wait()
			frame()
		}
		start()
		if settled != 1 || comp.Worktree != fixture.main.Path || root.pendingSessionBase(comp) == nil || root.conversationCreating[view.ID] {
			t.Fatal("uncertain base creation lost its draft, base or busy state")
		}
		start()
		if settled != 2 || len(requests) != 2 || requests[0].ID != requests[1].ID || requests[0].Command.Kind != "create_managed_worktree" {
			t.Fatalf("retry did not reuse its captured managed worktree request: %+v", requests)
		}
		input, valid := detail[protocol.CommandCreateManagedWorktreePayload](requests[1].Command.Data)
		if !valid || input.Source != fixture.main.ID || input.Branch != "feature" || input.Base == nil || *input.Base != base["commit"] || input.IncludeChanges {
			t.Fatalf("managed worktree request changed its captured base: %+v", input)
		}
		created := root.draftWorktree(comp, fixture.main.Project)
		if comp.Worktree != fixture.target.Path || root.pendingSessionBase(comp) != nil || created == nil || *created != fixture.target.ID {
			t.Fatal("the draft did not move into its created worktree before snapshot admission")
		}
	})
}

func testPluginLocationPicker(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newPluginLocationTestFixture(t)
		root := fixture.root
		first, second := root.App.Composer("draft/first"), root.App.Composer("draft/second")
		body, err := fixture.call(t, "sailry/ui.selectLocation", pluginLocationTestArguments(t, "1", map[string]any{"projects": true}))
		if err != nil || string(body) != "true" {
			t.Fatalf("captured draft did not request its retained project picker: %s, %v", body, err)
		}
		context := func(current, choice, id, path string) *composer.Context {
			return &composer.Context{
				Models: []composer.Model{{ID: "fixture", Name: "Fixture", Provider: "Provider"}},
				Projects: []composer.Project{
					{ID: fixture.main.Project.String(), Name: current, Host: root.controller.Target().String(), Worktrees: []composer.Worktree{{Branch: fixture.main.Path, Main: true}}},
					{ID: id, Name: choice, Host: root.controller.Target().String(), Worktrees: []composer.Worktree{{Branch: path, Main: true}}},
				},
				Repository: composer.Repository{Kind: composer.Directory},
			}
		}
		firstProject, secondProject := protocol.ProjectID{7}.String(), protocol.ProjectID{8}.String()
		firstContext := context("First current project", "First picker target", firstProject, "/fixture/first")
		secondContext := context("Second current project", "Second picker target", secondProject, "/fixture/second")
		first.Model, second.Model = "fixture", "fixture"
		tester := ui.NewTesterWith(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			ui.Column(view).Fill().Children(func() {
				ui.Column(view).Key("second").Children(func() {
					composer.View(view, second, secondContext, &conversationActions{r: root, c: view, cv: root.App.Data.Find("draft/second"), comp: second})
				})
				ui.Column(view).Key("first").Children(func() {
					composer.View(view, first, firstContext, &conversationActions{r: root, c: view, cv: root.App.Data.Find("draft/first"), comp: first})
				})
			})
		}, 1000, 1000, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}}) // no frost to rasterize
		tester.SetLocale("en")
		tester.Frame()
		tester.Advance(time.Second)
		tester.Frame()
		if !tester.HasText("First picker target") || tester.HasText("Second picker target") {
			t.Fatalf("the captured picker appeared in another simultaneous composer: %q", tester.Texts())
		}
		if err := tester.Click("First picker target"); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		if first.Project != firstProject || first.Worktree != "/fixture/first" ||
			second.Project != fixture.main.Project.String() || second.Worktree != fixture.main.Path || len(fixture.source.commands) != 0 {
			t.Fatal("project choice did not remain with its captured composer")
		}
		second.OpenProjectPicker()
		tester.Frame()
		tester.Advance(time.Second)
		tester.Frame()
		if !tester.HasText("Second picker target") || tester.HasText("First current project") {
			t.Fatalf("independent project pickers shared their retained open state: %q", tester.Texts())
		}
	})
}

func testPluginLocationReleases(t *testing.T) {
	t.Run("a closed owner cannot apply late removal confirmation", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			fixture.source.pending = make(chan link.Response, 1)
			project := fixture.root.workspaceFor(fixture.main)
			request := fixture.prepare(t, protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: fixture.main.ID}})
			completed := make(chan error, 1)
			go func() {
				_, err := fixture.state.native.Operations["sailry/ui.releaseWorktree"](
					fixture.root.ctx, pluginLocationTestArguments(t, fixture.main.ID, request.ID),
				)
				completed <- err
			}()
			frame := pluginLocationTestFrame(fixture.root)
			synctest.Wait()
			frame()
			synctest.Wait()
			if err := fixture.host.Close(); err != nil {
				t.Fatal(err)
			}
			fixture.source.pending <- link.Response{Output: protocol.Output{
				Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{
					ID: request.ID, Outcome: pluginLocationTestOutcome(protocol.Output{
						Kind: "worktree_removed", Data: protocol.OutputWorktreeRemovedPayload{ID: fixture.main.ID},
					}),
				},
			}}
			synctest.Wait()
			frame()
			synctest.Wait()
			select {
			case err := <-completed:
				if err == nil || fixture.root.workspace.projects[fixture.main.ID] != project {
					t.Fatalf("late removal released a closed owner's presentation: %v", err)
				}
			default:
				t.Fatal("closed removal owner retained its pending callback")
			}
		})
	})
	for _, name := range []string{"clean", "dirty", "pending file operation", "another Node is displayed"} {
		t.Run(name+" retains only the correct presentation owners", func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t)
				root := fixture.root
				project := root.workspaceFor(fixture.main)
				other := root.workspaceFor(fixture.target)
				project.panel.SetFile("source.go", "original")
				project.panel.Files.Show("source.go")
				if name == "dirty" {
					project.panel.FileChanged("source.go", "unsaved")
				}
				if name == "pending file operation" {
					project.uncertainOperations = map[string]protocol.Command{"write": {
						Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: fixture.main.ID, Path: "source.go", Text: "pending"},
					}}
				}
				cancelled, released := 0, 0
				project.cancel, project.release = func() { cancelled++ }, func() { released++ }
				root.workspace.selected, root.workspace.session = fixture.main.ID, protocol.SessionID{3}
				root.workspace.admitted[fixture.main.ID] = fixture.main
				root.App.Panel.Actions, root.App.Panel.Data = project.panel.Actions, project.data
				var displayed *viewmodel.Workspace
				if name == "another Node is displayed" {
					node := protocol.NodeID{9}
					controller := client.New(link.NewLocal(node, node, fixture.source))
					t.Cleanup(func() { _ = controller.Close() })
					root.bindings = map[string]*Runtime{node.String(): {parent: root, ctx: root.ctx, controller: controller}}
					root.App.Data.Find("draft/second").Node = node.String()
					root.App.Composer("draft/second").Host = node.String()
					displayed = &viewmodel.Workspace{}
					root.App.Panel.Data = displayed
				}
				request := fixture.prepare(t, protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: fixture.main.ID}})
				fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree_removed", Data: protocol.OutputWorktreeRemovedPayload{ID: fixture.main.ID}})
				body, err := fixture.call(t, "sailry/ui.releaseWorktree", pluginLocationTestArguments(t, fixture.main.ID, request.ID))
				if err != nil || string(body) != "null" || cancelled != 1 || released != 1 ||
					len(fixture.source.commands) != 1 || fixture.source.commands[0] != "inspect_request" {
					t.Fatalf("confirmed removal did not release its existing native owners exactly once: %s, %v, %v", body, err, fixture.source.commands)
				}
				if root.workspace.projects[fixture.target.ID] != other || root.workspace.selected != (protocol.WorktreeID{}) ||
					len(root.workspace.admitted) != 0 {
					t.Fatal("worktree cleanup changed another workspace or retained the released visible binding")
				}
				if displayed == nil && (root.App.Panel.Data == project.data || root.App.Panel.Actions != nil) ||
					displayed != nil && root.App.Panel.Data != displayed {
					t.Fatal("worktree cleanup replaced another Node's displayed workspace or kept its own released binding")
				}
				retained := root.workspace.projects[fixture.main.ID]
				preserve := name == "dirty" || name == "pending file operation"
				if !preserve && retained != nil || preserve && retained != project {
					t.Fatal("release discarded dirty or pending document ownership, or retained a clean project")
				}
				if name == "dirty" {
					if text, dirty := retained.panel.FileDraft("source.go"); text != "unsaved" || !dirty {
						t.Fatal("confirmed removal discarded a retained unsaved buffer")
					}
				}
			})
		})
	}
	for _, name := range []string{"wrong command", "wrong target", "missing write grant", "not admitted", "admitted", "unknown", "fault", "wrong output", "wrong worktree"} {
		t.Run(name+" cannot release presentation owners", func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t)
				project := fixture.root.workspaceFor(fixture.main)
				command := protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: fixture.main.ID}}
				initial := true
				switch name {
				case "wrong command":
					command = protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: *fixture.main.Project, Path: fixture.main.Path}}
				case "wrong target":
					command.Data = protocol.CommandRemoveWorktreePayload{Worktree: fixture.target.ID}
				case "missing write grant":
					info := fixture.root.packages.infos[fixture.state.scope.Package.Name]
					info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionReadWorktrees, protocol.PluginActionControlConversation}
					fixture.root.packages.infos[info.Summary.Name] = info
					fixture.state.locationOperations()
				default:
					initial = false
				}
				request := fixture.prepare(t, command)
				fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree_removed", Data: protocol.OutputWorktreeRemovedPayload{ID: fixture.main.ID}})
				switch name {
				case "not admitted":
					fixture.source.outcome.Kind, fixture.source.outcome.Data = "not_admitted", nil
				case "admitted", "unknown":
					fixture.source.outcome.Kind, fixture.source.outcome.Data = name, nil
				case "fault":
					failure := protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "fixture conflict"}
					fixture.source.outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Err: &failure}}
				case "wrong output":
					fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree", Data: fixture.main})
				case "wrong worktree":
					fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree_removed", Data: protocol.OutputWorktreeRemovedPayload{ID: fixture.target.ID}})
				}
				_, err := fixture.call(t, "sailry/ui.releaseWorktree", pluginLocationTestArguments(t, fixture.main.ID, request.ID))
				expectedReads := 1
				if initial {
					expectedReads = 0
				}
				if err == nil || len(fixture.source.commands) != expectedReads || fixture.root.workspace.projects[fixture.main.ID] != project {
					t.Fatalf("invalid removal released presentation or dispatched the original mutation: %v, %v", err, fixture.source.commands)
				}
			})
		})
	}
}
