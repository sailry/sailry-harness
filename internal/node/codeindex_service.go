package node

import (
	"context"
	"database/sql"
	"errors"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/codeindex"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// maxCodeQueries bounds the running queries and the remembered early
	// cancellations.
	maxCodeQueries    = 128
	codeQueryDeadline = 120 * time.Second
)

type codeQueryKey struct {
	caller  protocol.NodeID
	request protocol.RequestID
}

// codeIndexControls owns the code index service and its admitted queries.
type codeIndexControls struct {
	service *codeindex.Service

	mu      sync.Mutex
	running map[codeQueryKey]context.CancelFunc
	// cancelled remembers cancellations that arrived before their query.
	cancelled map[codeQueryKey]time.Time
}

func codeIndexCommand(kind string) bool {
	switch kind {
	case "search_code", "outline_code", "graph_code", "cancel_code_index_query", "read_code_index_settings", "save_code_index_settings":
		return true
	}
	return false
}

// initCodeIndex starts the index service on the workspace watch owner and
// applies the saved enablement.
func (node *Node) initCodeIndex(ctx context.Context) error {
	watch := func(ctx context.Context, root string, worktree protocol.WorktreeID) (link.Subscription, error) {
		return node.watches.observe(ctx, root, node.ID(), worktree, false, func(paths []string) {
			node.codeIndex.service.Observed(worktree, paths)
		})
	}
	node.codeIndex = &codeIndexControls{
		service:   codeindex.New(node.profile.Ownership.Path, watch),
		running:   map[codeQueryKey]context.CancelFunc{},
		cancelled: map[codeQueryKey]time.Time{},
	}
	return node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		settings, err := store.ReadCodeIndexSettings(ctx, db)
		if err == nil {
			node.codeIndex.service.Configure(settings.Enabled)
		}
		return err
	})
}

func (node *Node) closeCodeIndex() {
	if node.codeIndex != nil {
		node.codeIndex.service.Close()
	}
}

// refreshCodeIndex applies the committed enablement; an unreadable setting
// disables indexing.
func (node *Node) refreshCodeIndex() {
	ctx := context.WithoutCancel(node.ctx)
	enabled := false
	_ = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		settings, err := store.ReadCodeIndexSettings(ctx, db)
		enabled = err == nil && settings.Enabled
		return err
	})
	node.codeIndex.service.Configure(enabled)
}

// removeCodeIndexes deletes the derived caches of removed worktrees.
func (node *Node) removeCodeIndexes(worktrees ...protocol.WorktreeID) {
	for _, worktree := range worktrees {
		_ = node.codeIndex.service.Remove(worktree)
	}
}

// removeProjectCodeIndexes deletes the caches of a removed project. Project
// removal retains history and its worktree rows.
func (node *Node) removeProjectCodeIndexes(command protocol.Command) {
	input, ok := command.Data.(*protocol.CommandRemoveProjectPayload)
	if !ok {
		return
	}
	ctx := context.WithoutCancel(node.ctx)
	var worktrees []protocol.WorktreeID
	_ = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		rows, err := db.QueryContext(ctx, "SELECT id FROM worktrees WHERE project=?1", input.Expected.ID.String())
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var text string
			if err := rows.Scan(&text); err != nil {
				return err
			}
			id, err := protocol.ParseWorktreeID(text)
			if err != nil {
				return err
			}
			worktrees = append(worktrees, id)
		}
		return rows.Err()
	})
	node.removeCodeIndexes(worktrees...)
}

// begin admits one query in memory. Its context ends with the caller, the
// Node, a cancellation command or the query deadline.
func (controls *codeIndexControls) begin(ctx, closed context.Context, key codeQueryKey) (context.Context, func(), error) {
	controls.mu.Lock()
	defer controls.mu.Unlock()
	for known, at := range controls.cancelled {
		if time.Since(at) >= codeQueryDeadline {
			delete(controls.cancelled, known)
		}
	}
	if _, early := controls.cancelled[key]; early {
		delete(controls.cancelled, key)
		return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "code query was cancelled"}
	}
	if _, exists := controls.running[key]; exists || len(controls.running) >= maxCodeQueries {
		return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "code query capacity is exhausted"}
	}
	if ctx.Err() != nil || closed.Err() != nil {
		return nil, nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "code query was cancelled"}
	}
	operation, cancel := context.WithTimeout(ctx, codeQueryDeadline)
	detach := context.AfterFunc(closed, cancel)
	controls.running[key] = cancel
	return operation, func() {
		detach()
		cancel()
		controls.mu.Lock()
		delete(controls.running, key)
		controls.mu.Unlock()
	}, nil
}

func (controls *codeIndexControls) cancel(key codeQueryKey) {
	controls.mu.Lock()
	defer controls.mu.Unlock()
	if cancel := controls.running[key]; cancel != nil {
		cancel()
		return
	}
	if len(controls.cancelled) >= maxCodeQueries {
		oldest, found := key, false
		for known, at := range controls.cancelled {
			if !found || at.Before(controls.cancelled[oldest]) {
				oldest, found = known, true
			}
		}
		delete(controls.cancelled, oldest)
	}
	controls.cancelled[key] = time.Now()
}

func (node *Node) codeIndexIO(ctx context.Context, caller protocol.NodeID, request protocol.Request, root string, effect *workspaceCompletion) (protocol.Output, error) {
	switch input := request.Command.Data.(type) {
	case *protocol.CommandCancelCodeIndexQueryPayload:
		node.codeIndex.cancel(codeQueryKey{caller, input.Request})
		return protocol.Output{Kind: "code_index_query_cancelled", Data: protocol.OutputCodeIndexQueryCancelledPayload{Request: input.Request}}, nil
	case *protocol.CommandSaveCodeIndexSettingsPayload:
		effect.commit = func(ctx context.Context, db *sql.Conn, _ protocol.Output) (protocol.Output, []protocol.Event, error) {
			saved, err := store.SaveCodeIndexSettings(ctx, db, input.Settings, input.ExpectedRevision)
			return protocol.Output{Kind: "code_index_settings", Data: saved}, nil, err
		}
		effect.after = node.refreshCodeIndex
		return protocol.Output{}, nil
	}
	if request.Command.Kind == "read_code_index_settings" {
		var settings protocol.CodeIndexSettings
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			settings, err = store.ReadCodeIndexSettings(ctx, db)
			return err
		})
		return protocol.Output{Kind: "code_index_settings", Data: settings}, err
	}
	operation, release, err := node.codeIndex.begin(ctx, node.ctx, codeQueryKey{caller, request.ID})
	if err != nil {
		return protocol.Output{}, err
	}
	defer release()
	defer node.power.Hold()()
	var output protocol.Output
	switch input := request.Command.Data.(type) {
	case *protocol.CommandSearchCodePayload:
		output.Kind = "code_search"
		output.Data, err = node.codeIndex.service.Search(operation, input.Worktree, root, input.Query)
	case *protocol.CommandOutlineCodePayload:
		output.Kind = "code_outline"
		output.Data, err = node.codeIndex.service.Outline(operation, input.Worktree, root, input.Path)
	case *protocol.CommandGraphCodePayload:
		output.Kind = "code_graph"
		output.Data, err = node.codeIndex.service.Graph(operation, input.Worktree, root, input.Query)
	default:
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "code query expected"}
	}
	if err != nil {
		if errors.Is(operation.Err(), context.DeadlineExceeded) {
			err = &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "code query deadline exceeded"}
		}
		return protocol.Output{}, err
	}
	return output, nil
}

// codeToolValue shapes a code result for the model. Plugin flows and clients
// keep the full protocol result with byte ranges and per-symbol revisions.
func codeToolValue(output protocol.Output) (any, bool) {
	// symbol converts zero-based ranges with exclusive byte ends to one-based
	// inclusive lines.
	symbol := func(symbol protocol.CodeIndexSymbol, located bool) map[string]any {
		start := symbol.Range.StartLine + 1
		end := symbol.Range.EndLine
		if symbol.Range.EndColumn > 0 {
			end++
		}
		value := map[string]any{"name": symbol.Name, "kind": symbol.Kind, "depth": symbol.Depth, "start_line": start, "end_line": max(end, start)}
		if located {
			value["path"], value["language"] = symbol.Path, symbol.Language
		}
		if symbol.Signature != nil {
			value["signature"] = *symbol.Signature
		}
		return value
	}
	var data map[string]any
	switch page := output.Data.(type) {
	case protocol.CodeIndexSearchPage:
		symbols := make([]map[string]any, 0, len(page.Symbols))
		for _, item := range page.Symbols {
			symbols = append(symbols, symbol(item, true))
		}
		issues := page.Issues
		if issues == nil {
			issues = []protocol.CodeIndexIssue{}
		}
		data = map[string]any{"symbols": symbols, "index_revision": page.IndexRevision, "partial": page.Partial, "truncated": page.Truncated, "skipped": page.Skipped, "issues": issues}
	case protocol.CodeIndexOutline:
		symbols := make([]map[string]any, 0, len(page.Symbols))
		for _, item := range page.Symbols {
			symbols = append(symbols, symbol(item, false))
		}
		data = map[string]any{
			"path": page.Path, "language": page.Language, "file_revision": page.FileRevision, "index_revision": page.IndexRevision, "status": page.Status,
			"symbols": symbols, "capabilities": page.Capabilities, "partial": page.Partial, "truncated": page.Truncated,
		}
	default:
		return nil, false
	}
	return map[string]any{"kind": output.Kind, "data": data}, true
}
