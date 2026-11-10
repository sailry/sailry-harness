package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sync"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/agent/feed"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/files"
	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/node/host"
	"github.com/sailry/sailry-harness/internal/node/ports"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Node owns one profile, admitted operations and their committed event feed.
type Node struct {
	network           *link.Link
	networkUpdate     sync.Mutex
	networkDirectory  *networkDirectory
	networkProbes     *networkProbes
	hostSampler       *host.Sampler
	hostInstalls      *host.Installations
	ports             *ports.Service
	cancel            context.CancelFunc
	web               *workspaceWeb
	transfers         *files.Transfers
	git               *workspacegit.Service
	processes         *process.Service
	watches           *workspaceWatches
	environment       process.Environment
	processAfter      func(time.Duration) <-chan time.Time
	effects           sync.Mutex
	profile           *Profile
	ctx               context.Context
	mu                sync.Mutex
	stopped           bool
	jobs              chan commandJob
	work              sync.WaitGroup
	workerDone        chan struct{}
	closed            chan struct{}
	stopDone          chan struct{}
	stopErr           error
	agent             *engine.Runtime
	feed              *feed.Hub
	providerDirectory providers.Directory
	subscribersMu     sync.Mutex
	subscribers       map[*events]struct{}
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
	Network        *link.NetworkScope
	HostDeployment *host.DeploymentOptions
	Model          func(context.Context, model.Binding) (fantasy.LanguageModel, error)
	Fetch          web.FetchOptions
	Search         web.SearchOptions
	Environment    *process.Environment
	Git            workspacegit.Options
	Process        process.Options
}

func Start(ctx context.Context, path string) (*Node, error) {
	return StartWithOptions(ctx, path, Options{})
}

func StartWithOptions(ctx context.Context, path string, options Options) (*Node, error) {
	profile, err := OpenProfile(ctx, path)
	if err != nil {
		return nil, err
	}
	if err := profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return admission.Recover(ctx, db) }); err != nil {
		_ = profile.Close()
		return nil, err
	}
	nodeContext, cancel := context.WithCancel(context.WithoutCancel(ctx))
	value := &Node{cancel: cancel, profile: profile, ctx: nodeContext, jobs: make(chan commandJob), workerDone: make(chan struct{}), closed: make(chan struct{}), stopDone: make(chan struct{}), subscribers: make(map[*events]struct{})}
	if err := value.initWorkspace(ctx, options); err != nil {
		cancel()
		_ = profile.Close()
		return nil, err
	}
	value.feed = feed.New(value.ID())
	value.ports = ports.New(nodeContext, ports.Options{})
	value.networkDirectory = &networkDirectory{listing: protocol.DefaultNetworkListing()}
	value.networkProbes = &networkProbes{permits: make(chan struct{}, 4), now: time.Now}
	value.initHost()
	if options.HostDeployment != nil {
		value.hostInstalls = host.NewInstallations(*options.HostDeployment)
	}
	value.agent = engine.New(profile.Store.Node, value.ID(), profile.Ownership.Path, value.feed, value.publish, engine.Options{Model: options.Model, Attachment: value.capturedAttachment, AttachmentBytes: value.transfers.AttachmentBytes, ToolsForTurn: value.workspaceTools, HooksForTurn: value.turnHooks})
	if err := value.agent.Recover(ctx); err != nil {
		cancel()
		_ = value.closeWorkspace()
		_ = profile.Close()
		return nil, err
	}
	value.agent.Start()
	go value.run()
	if options.Network != nil {
		if err := value.bindNetwork(ctx, *options.Network); err != nil {
			_ = value.Stop(context.Background())
			return nil, err
		}
	}
	return value, nil
}

func (node *Node) ID() protocol.NodeID { return node.profile.Identity.ID() }

func (node *Node) Stop(ctx context.Context) error {
	node.mu.Lock()
	if !node.stopped {
		node.stopped = true
		close(node.closed)
		close(node.jobs)
		node.cancel()
		go func() {
			<-node.workerDone
			node.agent.Stop()
			var networkErr error
			if node.network != nil {
				networkErr = node.network.Close()
			}
			portErr := node.ports.Close()
			node.work.Wait()
			workspaceErr := node.closeWorkspace()
			err := errors.Join(networkErr, portErr, workspaceErr, node.profile.Close())
			node.mu.Lock()
			node.stopErr = err
			close(node.stopDone)
			node.mu.Unlock()
		}()
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

func (node *Node) Dispatch(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
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
	if workspaceCommand(request.Command.Kind) {
		return node.dispatchWorkspace(ctx, caller, request, encoded)
	}
	if hostCommand(request.Command.Kind) {
		return node.dispatchHost(ctx, caller, request, encoded)
	}
	if request.Command.Kind == "open_port" || request.Command.Kind == "cancel_port" {
		return node.dispatchPort(ctx, caller, request)
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
	// Admission is durable. Cancellation of its observer cannot cancel this job.
	node.jobs <- commandJob{caller: caller, request: request, admitted: true, completion: completion}
	return accepted, nil
}

func externalCommand(command protocol.Command) bool {
	if store.SupportsProviderCommand(command.Kind) {
		return true
	}
	switch command.Kind {
	case "create_project", "update_project", "remove_project", "register_worktree":
		return true
	case "create_session":
		input := command.Data.(*protocol.CommandCreateSessionPayload)
		return input.Project == nil && (input.Config == nil || input.Config.Resource != nil || input.Config.Assistant != nil)
	default:
		return false
	}
}

func supportedCommand(kind string) bool {
	if store.SupportsAgentCommand(kind) || store.SupportsProviderCommand(kind) || store.SupportsTeamRead(kind) || store.SupportsTeamCommand(kind) {
		return true
	}
	switch kind {
	case "snapshot", "inspect_request", "read_session", "read_power_settings", "read_telemetry_settings", "read_plugin_version":
		return true
	case "deliver_background", "continue_turn":
		return true
	case "create_project", "update_project", "remove_project", "register_worktree", "set_defaults", "create_session", "rename_session", "set_session_archived", "set_session_read", "archive_sessions", "remove_session", "remove_sessions", "set_session_order", "mark_notification_read", "mark_notifications_read", "dismiss_read_notifications", "dismiss_notifications", "publish_notification", "save_power_settings", "save_telemetry_settings":
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
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation authority is unavailable"}
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
		case "read_power_settings":
			value, err := store.ReadPower(ctx, db)
			output = protocol.Output{Kind: "power_settings", Data: value}
			return err
		case "read_telemetry_settings":
			value, err := store.ReadTelemetry(ctx, db)
			output = protocol.Output{Kind: "telemetry_settings", Data: value}
			return err
		case "read_plugin_version":
			value, err := store.ReadPluginVersion(ctx, db, request.Command.Data.(*protocol.CommandReadPluginVersionPayload).Package)
			output = protocol.Output{Kind: "plugin", Data: value}
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

func (node *Node) run() {
	defer close(node.workerDone)
	for job := range node.jobs {
		projectEffect := job.request.Command.Kind == "create_project" || job.request.Command.Kind == "update_project" || job.request.Command.Kind == "register_worktree" || job.request.Command.Kind == "remove_project"
		if projectEffect {
			node.effects.Lock()
		}

		prepared, prepareErr := node.prepare(job.request.Command)
		if prepareErr == nil && store.SupportsProviderCommand(job.request.Command.Kind) {
			job.provider, prepareErr = node.prepareProvider(node.ctx, job.caller, job.request)
		}
		result, err := node.execute(job, prepared, prepareErr)
		if err != nil {
			result = response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "operation result could not be recorded; inspect the target before retrying"})
		} else {
			node.publish()
			node.agent.Committed()
		}
		if projectEffect {
			node.effects.Unlock()
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
		if operationErr == nil && job.request.Plugin != nil && job.request.Command.Kind != "continue_turn" {
			operationErr = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "plugin invocation authority is unavailable"}
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
				output, changes, operationErr = store.ExecuteCommand(node.ctx, db, node.ID(), node.profile.Ownership.Path, job.request, prepared)
			}
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
	}
	return response(output, operationErr), err
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
