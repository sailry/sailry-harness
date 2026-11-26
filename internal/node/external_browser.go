package node

import (
	"context"
	"database/sql"
	"errors"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/engine"
	"github.com/sailry/sailry-harness/internal/node/externalbrowser"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
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
	owner := node.commandContext(commandJob{user: userFromContext(ctx), scope: ctx})
	operation, finish, err := node.beginPluginExecution(owner, request)
	if err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	result := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	// Admission is Node work without the Node mutex, as in dispatchWorkspace.
	if err := node.begin(); err != nil {
		finish()
		return link.Admission{}, err
	}
	if err := ctx.Err(); err != nil {
		node.work.Done()
		finish()
		return link.Admission{}, nodeFault(err)
	}
	var previous *link.Response
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Admit(ctx, db, caller, request, body)
		return err
	}); err != nil {
		node.work.Done()
		finish()
		return link.Admission{}, nodeFault(err)
	}
	if previous != nil {
		node.work.Done()
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
	go func() {
		defer node.work.Done()
		defer close(completion)
		output, effectErr := node.externalBrowser.Execute(operation, call)
		finish()
		completion <- node.settle(caller, request.ID, response(output, effectErr))
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
	// A stopped turn closes the session's browser once, whichever tool the
	// turn kept.
	var closing sync.Once
	closeStopped := func() error {
		if ctx.Err() != nil {
			closing.Do(func() { node.externalBrowser.Close(inv.Session.ID) })
		}
		return nil
	}
	host := nodeModuleHost{node: node}
	var tools []engine.Tool
	for _, info := range infos {
		if info.Extension == nil || info.Extension.Host == nil || node.hasReplacedPlugin(info.Summary.Name, info.Summary.Digest) {
			continue
		}
		owner := plugins.PackageReference(info)
		for _, declaration := range info.Extension.Tools {
			if declaration.Handler == nil || declaration.Handler.Operation == nil {
				continue
			}
			operation := *declaration.Handler.Operation
			if operation != protocol.ToolOperationReadExternalBrowser && operation != protocol.ToolOperationControlExternalBrowser {
				continue
			}
			name := plugins.ToolAlias(info.Summary.Name, declaration.Name)
			tool, err := host.buildExternalBrowserTool(inv, owner, moduleapi.ExternalBrowserToolConfig{
				Spec: moduleapi.ToolSpec{
					Name:       name,
					Operation:  operation,
					Parameters: browserToolSchema,
					Initial:    true,
				},
				Close: closeStopped,
			}, func(ctx context.Context) error {
				return node.pluginEnabled(ctx, owner.Name)
			})
			if err != nil {
				return nil, err
			}
			tools = append(tools, tool)
		}
	}
	return tools, nil
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
