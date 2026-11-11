package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"sync"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/externalbrowser"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// browserScope checks that a browser command names a conversation of this
// Node together with that conversation's worktree, and returns the worktree
// root.
func (node *Node) browserScope(ctx context.Context, id protocol.SessionID, worktree protocol.WorktreeID, plugin bool) (string, error) {
	var session protocol.Session
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		session, err = store.ReadSession(ctx, db, id)
		return err
	}); err != nil {
		return "", nodeFault(err)
	}
	if session.Worktree != worktree {
		return "", &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "resource session belongs to another worktree"}
	}
	root, err := node.workspaceRoot(ctx, worktree, plugin)
	return root, nodeFault(err)
}

// dispatchExternalBrowser admits a managed browser operation durably and runs
// it against the session's browser on this Node. A plugin call made for a
// turn stops with that turn; every other call stops with the Node.
func (node *Node) dispatchExternalBrowser(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	input, ok := request.Command.Data.(*protocol.CommandUseExternalBrowserPayload)
	if !ok {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid external browser operation"}
	}
	if _, err := node.externalBrowser.Browser(); err != nil {
		return link.Admission{}, err
	}
	if _, err := externalbrowser.Validate(input.Action, input.Arguments); err != nil {
		return link.Admission{}, err
	}
	root, err := node.browserScope(ctx, input.Session, input.Worktree, request.Plugin != nil)
	if err != nil {
		return link.Admission{}, err
	}
	operation, finish, err := node.beginPluginExecution(node.ctx, request)
	if err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	result := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	node.mu.Lock()
	defer node.mu.Unlock()
	if node.stopped {
		finish()
		return link.Admission{}, unavailable()
	}
	if err := ctx.Err(); err != nil {
		finish()
		return link.Admission{}, nodeFault(err)
	}
	var previous *link.Response
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Admit(ctx, db, caller, request, body)
		return err
	}); err != nil {
		finish()
		return link.Admission{}, nodeFault(err)
	}
	if previous != nil {
		finish()
		completion <- *previous
		close(completion)
		return result, nil
	}
	call := externalbrowser.Call{Request: request.ID, Session: input.Session, Root: root, Action: input.Action, Arguments: input.Arguments}
	if request.Plugin != nil {
		// Browser authority follows the package as it is now, so the decision
		// is repeated around browser startup.
		call.Check = func(ctx context.Context) error { return node.validatePluginRequest(ctx, caller, request) }
	}
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		defer close(completion)
		output, effectErr := node.externalBrowser.Execute(operation, call)
		finish()
		outcome := response(output, effectErr)
		if err := node.profile.Store.Node.Write(context.WithoutCancel(node.ctx), func(db *sql.Conn) error {
			return admission.Complete(context.WithoutCancel(node.ctx), db, caller, request.ID, outcome)
		}); err != nil {
			outcome = response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "operation result could not be recorded; inspect the target before retrying"})
		}
		completion <- outcome
	}()
	return result, nil
}

// externalBrowserTools binds the managed browser operations a turn's packages
// declare. The package's host script prepares each call and formats its
// result around these native tools. They are offered without an installed
// browser too: each call then answers the fault that names what to install.
func (node *Node) externalBrowserTools(ctx context.Context, inv *store.AgentInvocation) ([]engine.Tool, error) {
	if !node.externalBrowser.Available() {
		return nil, nil
	}
	var infos []protocol.PluginInfo
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		for _, reference := range inv.Turn.Plugins {
			info, err := store.ResolvePluginInfo(ctx, db, reference)
			if err != nil {
				return err
			}
			infos = append(infos, info)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	var schema map[string]any
	if err := json.Unmarshal(browserToolSchema, &schema); err != nil {
		return nil, err
	}
	// A stopped turn closes the session's browser once, whichever tool the
	// turn kept.
	var closing sync.Once
	closeStopped := func() error {
		if ctx.Err() != nil {
			closing.Do(func() { node.externalBrowser.Close(inv.Session.ID) })
		}
		return nil
	}
	var tools []engine.Tool
	for _, info := range infos {
		if info.Extension == nil || info.Extension.Host == nil {
			continue
		}
		for _, declaration := range info.Extension.Tools {
			if declaration.Handler == nil || declaration.Handler.Operation == nil {
				continue
			}
			operation := *declaration.Handler.Operation
			if operation != protocol.ToolOperationReadExternalBrowser && operation != protocol.ToolOperationControlExternalBrowser {
				continue
			}
			name := plugins.ToolAlias(info.Summary.Name, declaration.Name)
			tool := node.externalBrowserTool(inv, plugins.PackageReference(info), name, operation, schema)
			tool.Close = closeStopped
			tools = append(tools, tool)
		}
	}
	return tools, nil
}

func (node *Node) externalBrowserTool(inv *store.AgentInvocation, owner protocol.PluginReference, name string, operation protocol.ToolOperation, schema map[string]any) engine.Tool {
	readOnly := operation == protocol.ToolOperationReadExternalBrowser
	var mu sync.Mutex
	calls := map[string]*protocol.Request{}
	tool := engine.Tool{Definition: fantasy.FunctionTool{Name: name, InputSchema: schema}, SchemaJSON: browserToolSchema, ReadOnly: readOnly, Approval: !readOnly,
		Binding: &store.ApprovalBinding{Owner: owner, Tool: name, Operation: &operation}}
	tool.Prepare = func(_ context.Context, call canonical.ToolRequest) error {
		var input struct {
			Action    protocol.ExternalBrowserAction `json:"action"`
			Arguments json.RawMessage                `json:"arguments"`
		}
		decoder := json.NewDecoder(bytes.NewReader(call.Arguments))
		decoder.DisallowUnknownFields()
		if decoder.Decode(&input) != nil || input.Arguments == nil {
			return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid browser tool arguments"}
		}
		if input.Action.ReadOnly() != readOnly {
			return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "browser action does not match the declared operation"}
		}
		if _, err := externalbrowser.Validate(input.Action, input.Arguments); err != nil {
			return err
		}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "use_external_browser", Data: &protocol.CommandUseExternalBrowserPayload{
			Session: inv.Session.ID, Worktree: inv.Run.Worktree, Action: input.Action, Arguments: input.Arguments}})
		mu.Lock()
		calls[call.ID] = &request
		mu.Unlock()
		return nil
	}
	if !readOnly {
		tool.ConfigureApproval = func(ctx context.Context, db *sql.Conn, call canonical.ToolRequest, value protocol.ConversationApproval) (protocol.ConversationApproval, error) {
			mu.Lock()
			current := calls[call.ID]
			mu.Unlock()
			if current == nil {
				return value, errors.New("native tool was not prepared")
			}
			return store.ConfigureWorkspaceApproval(ctx, db, inv.Run.Turn, value, operation, current.Command, nil)
		}
		tool.Approved = func(call canonical.ToolRequest, _ protocol.ApprovalID, request protocol.RequestID) {
			mu.Lock()
			defer mu.Unlock()
			if current := calls[call.ID]; current != nil {
				current.ID = request
			}
		}
	}
	tool.Execute = func(ctx context.Context, call canonical.ToolRequest) engine.ToolResult {
		mu.Lock()
		current := calls[call.ID]
		delete(calls, call.ID)
		mu.Unlock()
		if current == nil {
			return workspaceToolFault(errors.New("native tool was not prepared"))
		}
		input := current.Command.Data.(*protocol.CommandUseExternalBrowserPayload)
		// The turn's context stops the call; an interrupted call closes the
		// session's browser. A turn keeps the package it started with, but
		// browser authority follows the package as it is now, so the
		// decision is repeated around browser startup.
		output, err := node.externalBrowser.Execute(ctx, externalbrowser.Call{Request: current.ID, Session: input.Session, Root: inv.Worktree.Path, Action: input.Action, Arguments: input.Arguments,
			Check: func(ctx context.Context) error { return node.pluginEnabled(ctx, owner.Name) }})
		if err != nil {
			return workspaceToolFault(err)
		}
		return engine.ToolResult{Value: map[string]any{"kind": output.Kind, "data": output.Data}}
	}
	return tool
}

// pluginEnabled refuses a browser tool of a package the user disabled or
// removed after the turn captured it.
func (node *Node) pluginEnabled(ctx context.Context, name string) error {
	return node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		current, err := store.ReadPlugin(ctx, db, name)
		if err != nil {
			return err
		}
		if current == nil || !current.Summary.Enabled {
			return &protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "plugin is disabled"}
		}
		return nil
	})
}

// externalBrowserEvents follows conversations that end. A removed
// conversation loses its browser and its browser profile, which holds its
// website sign-ins; an archived one loses only its running browser.
func (node *Node) externalBrowserEvents(changes []protocol.Event) {
	var removed, archived []protocol.SessionID
	for _, change := range changes {
		switch change.Kind {
		case "sessions_removed":
			sessions, _ := dispatchPayload[[]protocol.SessionID](change.Data)
			removed = append(removed, sessions...)
		case "session_changed":
			if session, valid := dispatchPayload[protocol.Session](change.Data); valid && session.Archived {
				archived = append(archived, session.ID)
			}
		}
	}
	if len(removed)+len(archived) == 0 {
		return
	}
	// The command lock may be held here and a close waits for the session's
	// running call, so it runs on its own. It touches only the browser
	// owner, which outlives the Node's other services.
	go func() {
		for _, session := range removed {
			node.externalBrowser.Remove(session)
		}
		for _, session := range archived {
			node.externalBrowser.Close(session)
		}
	}()
}

// removeExternalBrowserProfiles deletes the browser profiles of conversations
// that no longer exist: those removed while their profile could not be, or
// by a Node that stopped before it got to it. A read that fails keeps the
// profile for the next start.
func (node *Node) removeExternalBrowserProfiles(ctx context.Context) {
	for _, session := range node.externalBrowser.Profiles() {
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			_, err := store.ReadSession(ctx, db, session)
			return err
		})
		var missing *protocol.Fault
		if errors.As(err, &missing) && missing.Code == protocol.ErrorCodeNotFound {
			node.externalBrowser.Remove(session)
		}
	}
}
