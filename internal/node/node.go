package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Node owns one profile, admitted operations and their committed event feed.
type Node struct {
	profile       *Profile
	ctx           context.Context
	mu            sync.Mutex
	stopped       bool
	jobs          chan commandJob
	work          sync.WaitGroup
	workerDone    chan struct{}
	closed        chan struct{}
	stopDone      chan struct{}
	stopErr       error
	subscribersMu sync.Mutex
	subscribers   map[*events]struct{}
}

type commandJob struct {
	caller     protocol.NodeID
	request    protocol.Request
	body       []byte
	admitted   bool
	completion chan link.Response
}

var _ link.Handler = (*Node)(nil)

func Start(ctx context.Context, path string) (*Node, error) {
	profile, err := OpenProfile(ctx, path)
	if err != nil {
		return nil, err
	}
	if err := profile.Store.Node.Write(ctx, func(db *sql.Conn) error { return admission.Recover(ctx, db) }); err != nil {
		_ = profile.Close()
		return nil, err
	}
	value := &Node{profile: profile, ctx: context.WithoutCancel(ctx), jobs: make(chan commandJob), workerDone: make(chan struct{}), closed: make(chan struct{}), stopDone: make(chan struct{}), subscribers: make(map[*events]struct{})}
	go value.run()
	return value, nil
}

func (node *Node) ID() protocol.NodeID { return node.profile.Identity.ID() }

func (node *Node) Stop(ctx context.Context) error {
	node.mu.Lock()
	if !node.stopped {
		node.stopped = true
		close(node.closed)
		close(node.jobs)
		go func() {
			<-node.workerDone
			node.work.Wait()
			err := node.profile.Close()
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
	switch kind {
	case "snapshot", "inspect_request", "read_session", "read_power_settings", "read_telemetry_settings":
		return true
	case "create_project", "update_project", "remove_project", "register_worktree", "set_defaults", "create_session", "rename_session", "set_session_archived", "set_session_read", "archive_sessions", "remove_session", "remove_sessions", "set_session_order", "mark_notification_read", "mark_notifications_read", "dismiss_read_notifications", "dismiss_notifications", "publish_notification", "save_power_settings", "save_telemetry_settings":
		return true
	default:
		return false
	}
}

func (node *Node) read(ctx context.Context, caller protocol.NodeID, request protocol.Request) link.Response {
	var output protocol.Output
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
		default:
			return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "command is unavailable"}
		}
	})
	return response(output, err)
}

func (node *Node) run() {
	defer close(node.workerDone)
	for job := range node.jobs {
		prepared, prepareErr := node.prepare(job.request.Command)
		result, err := node.execute(job, prepared, prepareErr)
		if err != nil {
			result = response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "operation result could not be recorded; inspect the target before retrying"})
		} else {
			node.publish()
		}
		job.completion <- result
		close(job.completion)
	}
}

func (node *Node) execute(job commandJob, prepared store.PreparedPaths, prepareErr error) (link.Response, error) {
	var output protocol.Output
	var operationErr error
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
		var changes []protocol.Event
		operationErr = prepareErr
		if operationErr == nil {
			output, changes, operationErr = store.ExecuteCommand(node.ctx, db, node.ID(), node.profile.Ownership.Path, job.request, prepared)
		}
		if operationErr != nil {
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
