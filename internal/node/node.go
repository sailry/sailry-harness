package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"charm.land/fantasy"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/feed"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/computer"
	"github.com/sailry/sailry-harness/internal/node/externalbrowser"
	"github.com/sailry/sailry-harness/internal/node/files"
	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/node/host"
	knowledgeservice "github.com/sailry/sailry-harness/internal/node/knowledge/service"
	"github.com/sailry/sailry-harness/internal/node/mcp"
	"github.com/sailry/sailry-harness/internal/node/ports"
	"github.com/sailry/sailry-harness/internal/node/power"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/tasks"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
	"github.com/sailry/sailry-harness/internal/node/terminal"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/node/wsl"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Node owns one profile, admitted operations and their committed event feed.
type Node struct {
	pluginStatus      *pluginStatusService
	terminals         *terminal.Service
	auth              *authControls
	plugins           *pluginControls
	dispatch          *dispatchControls
	tasks             *tasks.Supervisor
	network           *link.Link
	networkUpdate     sync.Mutex
	networkDirectory  *networkDirectory
	networkProbes     *networkProbes
	hostSampler       *host.Sampler
	hostInstalls      *host.Installations
	wsl               *wsl.Service
	databases         databaseOperations
	ports             *ports.Service
	browsers          browserRegistry
	externalBrowser   *externalbrowser.Browsers
	localModels       *localModelControls
	power             *power.Service
	telemetry         *telemetry.Control
	logs              bool
	knowledge         *knowledgeservice.Service
	knowledgeAdjust   func(*knowledgeservice.Options) // replaces model capabilities in tests
	cancel            context.CancelFunc
	web               *workspaceWeb
	transfers         *files.Transfers
	git               *workspacegit.Service
	processes         *process.Service
	watches           *workspaceWatches
	codeIndex         *codeIndexControls
	computerOverlay   computer.Overlay
	environment       process.Environment
	processAfter      func(time.Duration) <-chan time.Time
	locks             workspaceLocks
	profile           *Profile
	ctx               context.Context
	mu                sync.Mutex
	stopped           bool
	lanes             [2]commandLane
	work              sync.WaitGroup
	closed            chan struct{}
	stopDone          chan struct{}
	stopErr           error
	grace             time.Duration // replaces shutdownGrace in tests
	agent             *engine.Runtime
	modelFactory      func(context.Context, model.Binding) (fantasy.LanguageModel, error)
	forward           func(context.Context, protocol.Request) (link.Admission, error) // sends an import to its paired target
	forwards          atomic.Int32
	forwarding        sync.Map      // request IDs whose forward is in flight
	mediaPoll         time.Duration // interval between video job reads
	feed              *feed.Hub
	providerDirectory providers.Directory
	subscribersMu     sync.Mutex
	subscribers       map[*events]struct{}
}

// commandLane is one ordered queue of admitted jobs and its worker. Project
// effects, which may clone a repository, have their own lane so that they do
// not delay turns and provider commands.
type commandLane struct {
	queue []commandJob
	wake  chan struct{}
	done  chan struct{}
}

const (
	generalLane = iota
	projectLane
)

func projectEffect(kind string) bool {
	return kind == "create_project" || kind == "register_project" || kind == "update_project" || kind == "register_worktree" || kind == "remove_project"
}

type commandJob struct {
	caller     protocol.NodeID
	request    protocol.Request
	body       []byte
	admitted   bool
	completion chan link.Response
	provider   *providers.Prepared
}

var _ link.Handler = (*Node)(nil)

// Options injects native process and clock boundaries for deterministic owners.
type Options struct {
	MCP            *mcp.Options
	Python         *python.Options
	DispatchClock  clockwork.Clock
	Network        *link.NetworkScope
	HostDeployment *host.DeploymentOptions
	Model          func(context.Context, model.Binding) (fantasy.LanguageModel, error)
	Fetch          web.FetchOptions
	Search         web.SearchOptions
	Environment    *process.Environment
	Git            workspacegit.Options
	Process        process.Options
	LocalModels    localmodels.Options
	Power          power.Backend
	Telemetry      telemetry.Options
	knowledge      func(*knowledgeservice.Options)
}

func Start(ctx context.Context, path string) (*Node, error) {
	return StartWithOptions(ctx, path, Options{})
}

func StartWithOptions(ctx context.Context, path string, options Options) (*Node, error) {
	profile, err := OpenProfile(ctx, path)
	if err != nil {
		return nil, err
	}
	if err := profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := admission.Recover(ctx, db); err != nil {
			return err
		}
		_, err := admission.Prune(ctx, db, time.Now())
		return err
	}); err != nil {
		_ = profile.Close()
		return nil, err
	}
	nodeContext, cancel := context.WithCancel(context.WithoutCancel(ctx))
	value := &Node{cancel: cancel, profile: profile, ctx: nodeContext, knowledgeAdjust: options.knowledge, closed: make(chan struct{}), stopDone: make(chan struct{}), subscribers: make(map[*events]struct{})}
	for index := range value.lanes {
		value.lanes[index] = commandLane{wake: make(chan struct{}, 1), done: make(chan struct{})}
	}
	// The keep-awake owner precedes every source of work it counts.
	if err := value.initPower(ctx, options.Power); err != nil {
		cancel()
		_ = profile.Close()
		return nil, err
	}
	value.tasks = tasks.New(nodeContext)
	if err := value.initWorkspace(ctx, options); err != nil {
		cancel()
		_ = profile.Close()
		return nil, err
	}
	// No command is admitted yet, so no upload is between its file and its row.
	value.collectAttachments(ctx)
	if err := value.initPlugins(ctx, options.MCP); err != nil {
		cancel()
		_ = value.tasks.Close(context.Background())
		_ = value.closeWorkspace()
		_ = profile.Close()
		return nil, err
	}
	if err := value.initPython(ctx, options.Python); err != nil {
		cancel()
		_ = value.closePlugins()
		_ = value.tasks.Close(context.Background())
		_ = value.closeWorkspace()
		_ = profile.Close()
		return nil, err
	}
	if err := value.initAuth(ctx); err != nil {
		cancel()
		_ = value.closePlugins()
		_ = value.tasks.Close(context.Background())
		_ = value.closeWorkspace()
		_ = profile.Close()
		return nil, err
	}
	if err := value.initTerminals(ctx); err != nil {
		cancel()
		value.closeAuth()
		_ = value.closePlugins()
		_ = value.tasks.Close(context.Background())
		_ = value.closeWorkspace()
		_ = profile.Close()
		return nil, err
	}
	value.feed = feed.New(value.ID())
	value.ports = ports.New(nodeContext, ports.Options{})
	value.externalBrowser = externalbrowser.New(externalbrowser.Options{Profile: profile.Ownership.Path, Node: value.ID(), Changed: func(root string) { value.watches.changed(root, nil) }})
	value.removeExternalBrowserProfiles(ctx)
	value.networkDirectory = &networkDirectory{listing: protocol.DefaultNetworkListing()}
	value.networkProbes = &networkProbes{permits: make(chan struct{}, 4), now: time.Now}
	value.initHost()
	if options.HostDeployment != nil {
		value.hostInstalls = host.NewInstallations(*options.HostDeployment)
	}
	value.modelFactory = options.Model
	value.mediaPoll = model.MediaVideoPoll
	value.telemetry = newTelemetry(options.Telemetry)
	value.agent = engine.New(profile.Store.Node, value.ID(), profile.Ownership.Path, value.feed, value.publish, engine.Options{StopCommands: func(session protocol.SessionID) { value.processes.StopAll(session) }, Telemetry: value.telemetry, Working: value.power.Hold, Python: value.plugins.python, Model: value.newModel, AuthorizeProvider: value.authorizeTurnProvider, Attachment: value.capturedAttachment, AttachmentBytes: value.transfers.AttachmentBytes, ToolsForTurn: value.workspaceTools, HooksForTurn: value.turnHooks, AfterTurn: value.afterTurn})
	if err := value.agent.Recover(ctx); err != nil {
		cancel()
		value.closeAuth()
		_ = value.terminals.Close()
		_ = value.closePlugins()
		_ = value.tasks.Close(context.Background())
		_ = value.closeWorkspace()
		_ = profile.Close()
		return nil, err
	}
	if err := value.initTelemetry(ctx); err != nil {
		cancel()
		value.closeAuth()
		_ = value.terminals.Close()
		_ = value.closePlugins()
		_ = value.tasks.Close(context.Background())
		_ = value.closeWorkspace()
		_ = profile.Close()
		return nil, err
	}
	if err := value.initLocalModels(ctx, options.LocalModels); err != nil {
		cancel()
		value.closeAuth()
		_ = value.terminals.Close()
		_ = value.closePlugins()
		_ = value.tasks.Close(context.Background())
		_ = value.closeWorkspace()
		_ = profile.Close()
		return nil, err
	}
	value.agent.Start()
	for index := range value.lanes {
		go value.run(&value.lanes[index])
	}
	go value.pruneLedger()
	value.initDispatch(options.DispatchClock)
	value.initPluginStatus()
	value.startMCP()
	value.sweepSessionWorkspaces()
	if options.Network != nil {
		if err := value.bindNetwork(ctx, *options.Network); err != nil {
			_ = value.Stop(context.Background())
			return nil, err
		}
	}
	return value, nil
}

func (node *Node) ID() protocol.NodeID { return node.profile.Identity.ID() }

// shutdownGrace is the deadline the Rust Node gives its tasks to stop.
const shutdownGrace = 5 * time.Second

// Stop cancels every worker and waits for the Node to close. Work that
// ignores cancellation is abandoned at the shutdown deadline: Stop then
// returns core.ErrShutdownTimeout naming it, with the profile released, and
// the caller exits. Go cannot abort a goroutine, so the exit is what ends it.
func (node *Node) Stop(ctx context.Context) error {
	node.mu.Lock()
	if !node.stopped {
		node.stopped = true
		close(node.closed)
		for index := range node.lanes {
			node.signal(&node.lanes[index])
		}
		node.cancel()
		grace := node.grace
		if grace == 0 {
			grace = shutdownGrace
		}
		go node.shutdown(grace)
	}
	done := node.stopDone
	node.mu.Unlock()
	select {
	case <-done:
		node.mu.Lock()
		err := node.stopErr
		node.mu.Unlock()
		return err
	case <-ctx.Done():
		return ctx.Err()
	}
}

type stopStage struct {
	name string
	run  func() error
}

func (node *Node) shutdown(grace time.Duration) {
	deadline, release := context.WithTimeout(context.Background(), grace)
	defer release()
	quiet := func(run func()) func() error { return func() error { run(); return nil } }
	// Each stage waits for the work it names. The order is the dependency
	// order: commands, then turns, then what they use, then storage.
	stages := []stopStage{
		{"command lanes", quiet(func() {
			for index := range node.lanes {
				<-node.lanes[index].done
			}
		})},
		{"plugin status", quiet(node.closePluginStatus)},
		{"agent turns", quiet(node.agent.Stop)},
		{"logins", quiet(node.closeAuth)},
		{"terminals", node.terminals.Close},
		{"network", func() error {
			if node.network == nil {
				return nil
			}
			return node.network.Close()
		}},
		{"port forwards", node.ports.Close},
		// Supervised tasks may finish until the deadline and are then cancelled.
		{"supervised tasks", func() error { return node.tasks.Close(deadline) }},
		{"knowledge index", quiet(node.knowledge.Close)},
		{"local models", quiet(node.closeLocalModels)},
		{"external browsers", quiet(node.externalBrowser.Shutdown)},
		{"plugins and MCP servers", node.closePlugins},
		{"requests in flight", quiet(node.work.Wait)},
		// No request holds a package version or a settings revision any more.
		{"plugin collection", quiet(func() { node.collectPlugins(context.Background()) })},
		{"telemetry", quiet(func() {
			node.detachLogging()
			node.telemetry.Close()
			node.power.Close()
		})},
		{"commands, transfers and watchers", node.closeWorkspace},
		// Every transfer worker has ended and storage is still open.
		{"attachment collection", quiet(func() { node.collectAttachments(context.Background()) })},
		{"storage", node.profile.Close},
	}
	var reached atomic.Int32
	finished := make(chan error, 1)
	go func() {
		var failures []error
		for index, stage := range stages {
			reached.Store(int32(index))
			failures = append(failures, stage.run())
		}
		finished <- errors.Join(failures...)
	}()
	var err error
	select {
	case err = <-finished:
	case <-deadline.Done():
		index := int(reached.Load())
		skipped := make([]string, 0, len(stages))
		for _, stage := range stages[index+1:] {
			skipped = append(skipped, stage.name)
		}
		slog.Error("Node shutdown deadline elapsed", "waiting", stages[index].name, "skipped", strings.Join(skipped, ", "))
		// The stages above keep running until the process exits. Releasing
		// the profile here lets the next launch start; what they leave behind
		// is what a crash leaves, and startup recovery clears it.
		err = errors.Join(fmt.Errorf("%w: %s did not stop", core.ErrShutdownTimeout, stages[index].name), node.profile.Abandon())
	}
	node.mu.Lock()
	node.stopErr = err
	close(node.stopDone)
	node.mu.Unlock()
}

func (node *Node) begin() error {
	node.mu.Lock()
	defer node.mu.Unlock()
	if node.stopped {
		return unavailable()
	}
	node.work.Add(1)
	return nil
}

func (node *Node) Snapshot(ctx context.Context) (protocol.Snapshot, error) {
	if err := node.begin(); err != nil {
		return protocol.Snapshot{}, err
	}
	defer node.work.Done()
	var value protocol.Snapshot
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error { var err error; value, err = store.Snapshot(ctx, db, node.ID()); return err })
	return value, nodeFault(err)
}

func (node *Node) admitCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
	if request.Target != node.ID() || request.Version != protocol.VERSION {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "Node or protocol version mismatch"}
	}
	encoded, err := json.Marshal(request)
	if err != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid command"}
	}
	if len(encoded) > protocol.MAX_FRAME_BYTES {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "request exceeds the frame limit"}
	}
	// Capture controller-owned maps and pointers before returning the receipt.
	if err := json.Unmarshal(encoded, &request); err != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid command"}
	}
	if request.Plugin != nil {
		if request.Command.Durable() {
			if accepted, handled, err := node.replayPluginAdmission(ctx, caller, request, encoded); handled || err != nil {
				return accepted, err
			}
		}
		if err := node.validatePluginRequest(ctx, caller, request); err != nil {
			return link.Admission{}, nodeFault(err)
		}
	}
	if accepted, handled, err := node.dispatchPluginMessageCommand(ctx, caller, request, encoded); handled {
		return accepted, err
	}
	if pluginCommand(request.Command.Kind) {
		return node.dispatchPlugins(ctx, caller, request, encoded)
	}
	if workspaceCommand(request.Command.Kind) {
		return node.dispatchWorkspace(ctx, caller, request, encoded)
	}
	if request.Command.Kind == "decide" {
		return node.dispatchDecision(ctx, request)
	}
	if request.Command.Kind == "send_diagnostics" {
		return node.dispatchDiagnostics(ctx, request)
	}
	if request.Command.Kind == "count_feature" {
		return node.dispatchFeature(caller, request)
	}
	if request.Command.Kind == "synthesize_speech" {
		return node.dispatchSpeech(ctx, request)
	}
	if localModelCommand(request.Command.Kind) {
		return node.dispatchLocalModels(ctx, caller, request, encoded)
	}
	if knowledgeCommand(request.Command.Kind) {
		return node.dispatchKnowledge(ctx, caller, request, encoded)
	}
	if computerCommand(request.Command.Kind) {
		return node.dispatchComputer(ctx, caller, request)
	}
	if hostCommand(request.Command.Kind) {
		return node.dispatchHost(ctx, caller, request, encoded)
	}
	if backupCommand(request.Command.Kind) {
		return node.dispatchBackup(ctx, caller, request, encoded)
	}
	if request.Command.Kind == "open_port" || request.Command.Kind == "cancel_port" {
		return node.dispatchPort(ctx, caller, request)
	}
	if request.Command.Kind == "use_external_browser" {
		return node.dispatchExternalBrowser(ctx, caller, request, encoded)
	}
	if request.Command.Kind == "use_browser" || request.Command.Kind == "complete_browser" {
		return node.dispatchBrowser(ctx, caller, request, encoded)
	}
	if input, ok := request.Command.Data.(*protocol.CommandCreateSessionAtPayload); ok && input.Target != node.ID() {
		return node.dispatchSessionForward(ctx, caller, request, encoded)
	}
	supported := supportedCommand(request.Command.Kind)
	durable := request.Command.Durable()
	if !supported {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "command is unavailable"}
	}
	completion := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: durable}, Completion: completion}
	if !durable {
		if err := node.begin(); err != nil {
			return link.Admission{}, err
		}
		go func() { defer node.work.Done(); defer close(completion); completion <- node.read(ctx, caller, request) }()
		return accepted, nil
	}
	node.mu.Lock()
	defer node.mu.Unlock()
	if node.stopped {
		return link.Admission{}, unavailable()
	}
	if err := ctx.Err(); err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if !externalCommand(request.Command) {
		result, err := node.execute(commandJob{caller: caller, request: request, body: encoded}, store.PreparedPaths{}, nil)
		if err != nil {
			return link.Admission{}, nodeFault(err)
		}
		node.publish()
		node.agent.Committed()
		completion <- result
		close(completion)
		return accepted, nil
	}
	var previous *link.Response
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Admit(ctx, db, caller, request, encoded)
		return err
	})
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if previous != nil {
		completion <- *previous
		close(completion)
		return accepted, nil
	}
	// Admission is durable. Cancellation of its observer cannot cancel this
	// job, and a busy worker cannot hold up the admission of the next one.
	lane := &node.lanes[generalLane]
	if projectEffect(request.Command.Kind) {
		lane = &node.lanes[projectLane]
	}
	lane.queue = append(lane.queue, commandJob{caller: caller, request: request, admitted: true, completion: completion})
	node.signal(lane)
	return accepted, nil
}

func (node *Node) signal(lane *commandLane) {
	select {
	case lane.wake <- struct{}{}:
	default:
	}
}

// next returns the lane's oldest admitted job. After Stop it still returns
// the jobs admitted before it, which then fail on the cancelled Node context.
func (node *Node) next(lane *commandLane) (commandJob, bool) {
	for {
		node.mu.Lock()
		if len(lane.queue) > 0 {
			job := lane.queue[0]
			lane.queue[0] = commandJob{}
			lane.queue = lane.queue[1:]
			node.mu.Unlock()
			return job, true
		}
		stopped := node.stopped
		node.mu.Unlock()
		if stopped {
			return commandJob{}, false
		}
		<-lane.wake
	}
}

func externalCommand(command protocol.Command) bool {
	if store.SupportsProviderCommand(command.Kind) {
		return true
	}
	switch command.Kind {
	case "submit_turn", "submit_task", "continue_turn", "compact_context", "start_queued_turn", "send_queued_turn", "replace_turn":
		return true
	case "create_project", "register_project", "update_project", "remove_project", "register_worktree":
		return true
	case "create_session":
		input := command.Data.(*protocol.CommandCreateSessionPayload)
		return input.Project == nil && (input.Config == nil || input.Config.Resource != nil || input.Config.Assistant != nil)
	case "import_session":
		input := command.Data.(*protocol.SessionImport)
		return input.Project == nil && (input.Config.Resource != nil || input.Config.Assistant != nil)
	default:
		return false
	}
}

func supportedCommand(kind string) bool {
	if store.SupportsAgentCommand(kind) || store.SupportsProviderCommand(kind) || store.SupportsTeamRead(kind) || store.SupportsTeamCommand(kind) {
		return true
	}
	switch kind {
	case "snapshot", "inspect_request", "read_session", "list_associated_sessions", "read_power_settings", "read_telemetry_settings", "read_plugin_version", "read_activity_catalog", "read_activity",
		"read_media_settings", "list_media_models":
		return true
	case "register_project", "report_diagnostic", "save_media_settings":
		return true
	case "deliver_background", "continue_turn":
		return true
	case "create_project", "update_project", "remove_project", "register_worktree", "set_defaults", "create_session", "import_session", "bind_connection_session", "list_databases", "save_database", "remove_database", "rename_session", "set_session_archived", "set_session_read", "archive_sessions", "remove_session", "remove_sessions", "set_session_order", "mark_notification_read", "mark_notifications_read", "dismiss_read_notifications", "dismiss_notifications", "publish_notification", "save_power_settings", "reset_telemetry_identity", "save_telemetry_settings":
		return true
	default:
		return false
	}
}

func (node *Node) read(ctx context.Context, caller protocol.NodeID, request protocol.Request) link.Response {
	if input, ok := request.Command.Data.(*protocol.CommandReadToolOutputPayload); ok && input.Stream != nil {
		if request.Plugin != nil {
			return response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation authority is unavailable"})
		}
		var session protocol.SessionID
		var id protocol.RequestID
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			session, id, err = store.ResolveToolCommandStream(ctx, db, caller, *input)
			return err
		})
		if err != nil {
			return response(protocol.Output{}, err)
		}
		page, err := node.processes.Page(session, id, *input.Stream, input.Offset, input.Limit)
		return response(protocol.Output{Kind: "command_output_page", Data: page}, err)
	}

	if store.SupportsProviderCommand(request.Command.Kind) {
		if err := node.renewCommandProvider(ctx, request); err != nil {
			return response(protocol.Output{}, err)
		}
		return node.readProvider(ctx, caller, request)
	}
	var output protocol.Output
	var sequence uint64
	switch input := request.Command.Data.(type) {
	case *protocol.CommandReadConversationPayload:
		unlock := node.agent.LockPublication()
		defer unlock()
		sequence = node.feed.Sequence(input.Session)
	case *protocol.CommandReadTurnPayload:
		unlock := node.agent.LockPublication()
		defer unlock()
		sequence = node.feed.Sequence(input.Session)
	}
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		if request.Plugin != nil {
			if err := node.validatePluginRequestDB(ctx, db, caller, request); err != nil {
				return err
			}
		}
		switch request.Command.Kind {
		case "snapshot":
			value, err := store.Snapshot(ctx, db, node.ID())
			output = protocol.Output{Kind: "snapshot", Data: value}
			return err
		case "inspect_request":
			input := request.Command.Data.(*protocol.CommandInspectRequestPayload)
			value, err := admission.Inspect(ctx, db, caller, input.ID, input.Digest)
			output = protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: input.ID, Outcome: value}}
			return err
		case "read_session":
			value, err := store.ReadSession(ctx, db, request.Command.Data.(*protocol.CommandReadSessionPayload).Session)
			output = protocol.Output{Kind: "session", Data: value}
			return err
		case "list_associated_sessions":
			value, err := store.ListAssociatedSessions(ctx, db, request)
			output = protocol.Output{Kind: "associated_sessions", Data: value}
			return err
		case "read_power_settings":
			value, err := store.ReadPower(ctx, db)
			output = protocol.Output{Kind: "power_settings", Data: value}
			return err
		case "read_telemetry_settings":
			value, err := store.ReadTelemetry(ctx, db)
			output = protocol.Output{Kind: "telemetry_settings", Data: value}
			return err
		case "read_media_settings":
			value, err := store.ReadMediaSettings(ctx, db)
			output = protocol.Output{Kind: "media_settings", Data: value}
			return err
		case "list_media_models":
			value, err := store.MediaModels(ctx, db)
			output = protocol.Output{Kind: "media_models", Data: value}
			return err
		case "read_plugin_version":
			value, err := store.ReadPluginVersion(ctx, db, request.Command.Data.(*protocol.CommandReadPluginVersionPayload).Package)
			output = protocol.Output{Kind: "plugin", Data: value}
			return err
		case "list_databases":
			profiles, err := store.ListDatabases(ctx, db)
			if err == nil && request.Plugin != nil {
				profiles, err = store.FilterPluginDatabases(ctx, db, *request.Plugin, profiles)
			}
			output = protocol.Output{Kind: "database_profiles", Data: profiles}
			return err
		case "read_activity_catalog":
			value, err := store.ReadActivityCatalog(ctx, db, node.ID())
			output = protocol.Output{Kind: "activity_catalog", Data: value}
			return err
		case "read_activity":
			input := request.Command.Data.(*protocol.CommandReadActivityPayload)
			value, err := store.ReadActivityPreviews(ctx, db, input.Sessions)
			output = protocol.Output{Kind: "activity", Data: value}
			return err
		default:
			if store.SupportsTeamRead(request.Command.Kind) {
				var err error
				output, err = store.ReadTeamCommand(ctx, db, request.Command)
				return err
			}
			if store.SupportsAgentCommand(request.Command.Kind) {
				var err error
				output, err = store.ReadAgentCommandForCaller(ctx, db, node.ID(), caller, request.Command)
				return err
			}
			return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "command is unavailable"}
		}
	})
	switch history := output.Data.(type) {
	case protocol.ConversationHistory:
		history.Sequence = sequence
		output.Data = history
	case protocol.ConversationTurnHistory:
		history.Sequence = sequence
		output.Data = history
	}
	return response(output, err)
}

func (node *Node) run(lane *commandLane) {
	defer close(lane.done)
	for job, ok := node.next(lane); ok; job, ok = node.next(lane) {
		projectEffect := projectEffect(job.request.Command.Kind)
		prepared, prepareErr := node.prepare(job.request.Command)
		if prepareErr == nil && job.request.Command.Kind == "remove_project" {
			prepared.TerminalsClosed, prepareErr = node.closeProjectResources(job.request.Command)
		}
		if prepareErr == nil {
			prepareErr = node.renewCommandProvider(node.ctx, job.request)
		}
		if prepareErr == nil && store.SupportsProviderCommand(job.request.Command.Kind) {
			job.provider, prepareErr = node.prepareProvider(node.ctx, job.caller, job.request)
		}
		// A clone and the checks above run unlocked. Only recording which
		// roots exist excludes workspace effects.
		registered := func() {}
		if projectEffect {
			registered = node.locks.registration()
		}
		result, err := node.execute(job, prepared, prepareErr)
		registered()
		if err != nil {
			result = node.unapplied(job.caller, job.request.ID, err)
		} else {
			node.publish()
			node.agent.Committed()
			if job.request.Command.Kind == "remove_project" && result.Fault == nil {
				node.removeProjectCodeIndexes(job.request.Command)
			}
		}
		job.completion <- result
		close(job.completion)
	}
}

func (node *Node) execute(job commandJob, prepared store.PreparedPaths, prepareErr error) (link.Response, error) {
	unlock := node.agent.LockDecision(job.request.Command)
	defer unlock()
	publication := node.agent.LockPublication()
	defer publication()
	var output protocol.Output
	var operationErr error
	var changes []protocol.Event
	var reviewChanges []store.AgentReviewChange
	var previous *link.Response
	err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
		if !job.admitted {
			var err error
			previous, err = admission.Admit(node.ctx, db, job.caller, job.request, job.body)
			if err != nil || previous != nil {
				return err
			}
		}
		if _, err := db.ExecContext(node.ctx, "SAVEPOINT sailry_command"); err != nil {
			return err
		}
		operationErr = prepareErr
		if operationErr == nil && job.request.Plugin != nil {
			operationErr = node.validatePluginRequestDB(node.ctx, db, job.caller, job.request)
		}
		if operationErr == nil {
			switch {
			case store.SupportsTeamCommand(job.request.Command.Kind):
				output, changes, operationErr = store.ExecuteTeamCommand(node.ctx, db, node.ID(), job.request.Command)
			case job.request.Command.Kind == "deliver_background" || job.request.Command.Kind == "continue_turn":
				output, changes, operationErr = store.ExecuteBackgroundCommand(node.ctx, db, node.ID(), job.caller, job.request, time.Now())
			case store.SupportsAgentCommand(job.request.Command.Kind):
				output, changes, operationErr = store.ExecuteAgentCommand(node.ctx, db, node.ID(), node.profile.Ownership.Path, job.caller, job.request, time.Now())
			case store.SupportsProviderCommand(job.request.Command.Kind):
				output, changes, operationErr = store.ExecuteProviderCommand(node.ctx, db, node.ID(), job.caller, job.request, job.provider)
			default:
				output, changes, operationErr = store.ExecuteCommand(node.ctx, db, node.ID(), job.caller, node.profile.Ownership.Path, job.request, prepared)
			}
		}
		if operationErr == nil && job.request.Command.Kind == "continue_turn" {
			changes, operationErr = node.followKnowledgeStorage(node.ctx, db, job.request, changes)
		}
		if operationErr == nil {
			var reviewEvents []protocol.Event
			reviewChanges, reviewEvents, operationErr = store.ApplyReviewCommand(node.ctx, db, job.request, output, time.Now())
			changes = append(changes, reviewEvents...)
		}
		if operationErr != nil {
			reviewChanges = nil
			if _, err := db.ExecContext(node.ctx, "ROLLBACK TO sailry_command"); err != nil {
				return err
			}
			changes = nil
		}
		if _, err := db.ExecContext(node.ctx, "RELEASE sailry_command"); err != nil {
			return err
		}
		if _, err := store.RecordEvents(node.ctx, db, node.ID(), changes); err != nil {
			return err
		}
		return admission.Complete(node.ctx, db, job.caller, job.request.ID, response(output, operationErr))
	})
	if previous != nil {
		return *previous, err
	}
	if err == nil {
		for _, review := range reviewChanges {
			node.feed.Publish(review.Session, review.Change)
		}
		node.conversationEvents(changes)
		node.systemEvents(changes)
		if operationErr == nil {
			node.refreshKnowledgeStorage(job.request.Command, nil)
		}
	}
	return response(output, operationErr), err
}

// settle stores the result of an admitted request whose own transaction did
// not store it, and answers with that result. The write is repeated until the
// Node stops; only then, or when the store cannot confirm a commit, is the
// answer OutcomeUnknown, and the next start marks the request unknown.
func (node *Node) settle(caller protocol.NodeID, id protocol.RequestID, result link.Response) link.Response {
	if err := admission.Settle(node.ctx, node.profile.Store.Node.Write, caller, id, result); err != nil {
		return response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "operation result could not be recorded; inspect the request before retrying"})
	}
	return result
}

// unapplied settles an admitted request whose whole effect was in the
// transaction that failed with cause. The store reports an error only for a
// transaction it did not commit, so the request changed nothing and its
// failure is its result. The exception is a commit that could not be
// confirmed: that request stays admitted until the next start.
func (node *Node) unapplied(caller protocol.NodeID, id protocol.RequestID, cause error) link.Response {
	failure := nodeFault(cause).(*protocol.Fault)
	if failure.Code == protocol.ErrorCodeOutcomeUnknown {
		return link.Response{Fault: failure}
	}
	return node.settle(caller, id, link.Response{Fault: failure})
}

// pruneLedger removes expired requests once a day while the Node runs.
func (node *Node) pruneLedger() {
	ticker := time.NewTicker(admission.PruneInterval)
	defer ticker.Stop()
	for {
		select {
		case <-node.ctx.Done():
			return
		case <-ticker.C:
		}
		if node.begin() != nil {
			return
		}
		err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
			_, err := admission.Prune(node.ctx, db, time.Now())
			return err
		})
		node.work.Done()
		if err != nil && node.ctx.Err() == nil {
			slog.Warn("request ledger could not be pruned", "code", nodeFault(err).(*protocol.Fault).Code)
		}
	}
}

func (node *Node) publish() {
	node.subscribersMu.Lock()
	defer node.subscribersMu.Unlock()
	for subscriber := range node.subscribers {
		select {
		case subscriber.wake <- struct{}{}:
		default:
		}
	}
}

func unavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Node service is unavailable"}
}

func nodeFault(err error) error {
	if err == nil {
		return nil
	}
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	if errors.Is(err, context.Canceled) {
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "operation was cancelled"}
	}
	return &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "Node storage operation failed"}
}

func response(output protocol.Output, err error) link.Response {
	if err == nil {
		return link.Response{Output: output}
	}
	return link.Response{Fault: nodeFault(err).(*protocol.Fault)}
}
