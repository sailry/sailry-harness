package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchCallbackScope(t *testing.T) {
	t.Parallel()
	for _, transaction := range []bool{false, true} {
		t.Run(fmt.Sprintf("save_and_update_transaction_%t", transaction), func(t *testing.T) {
			t.Parallel()
			f := newDispatchScopeFixture(t)
			ctx := t.Context()
			save := func(handler protocol.DispatchHandler, prefix bool) error {
				return f.storage.Node.Write(ctx, func(db *sql.Conn) error {
					action := protocol.DispatchCommand{Kind: "save_handler", Data: &handler}
					if !transaction {
						return f.dispatch(ctx, db, action)
					}
					operations := []protocol.PluginTransactionOperation{}
					if prefix {
						first := f.handler
						first.Name = "before-rejected-save"
						operations = append(operations, protocol.PluginTransactionOperation{Kind: "dispatch", Data: protocol.DispatchCommand{Kind: "save_handler", Data: &first}})
					}
					operations = append(operations, protocol.PluginTransactionOperation{Kind: "dispatch", Data: action})
					request := protocol.Request{Plugin: &f.scope, Command: protocol.Command{Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{Operations: operations}}}
					if err := CheckPluginRequest(ctx, db, request, f.info); err != nil {
						return err
					}
					_, _, err := ExecutePluginStorageCommand(ctx, db, f.node, f.storage.Profile, f.node, request)
					return err
				})
			}
			read := func() []protocol.DispatchHandler {
				t.Helper()
				var handlers []protocol.DispatchHandler
				if err := f.storage.Node.Read(ctx, func(db *sql.Conn) error {
					var err error
					handlers, err = dispatchList[protocol.DispatchHandler](ctx, db, "dispatch_handlers", f.scope.Package.Name)
					return err
				}); err != nil {
					t.Fatal(err)
				}
				return handlers
			}
			for _, scope := range []protocol.DispatchScope{{}, {Worktree: &f.other}} {
				handler := f.handler
				handler.Callback.Scope = scope
				if err := save(handler, true); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
					t.Fatalf("scope-widening create was not refused: %v", err)
				}
				if handlers := read(); len(handlers) != 0 {
					t.Fatalf("rejected create retained handlers: %+v", handlers)
				}
			}
			if err := save(f.handler, false); err != nil {
				t.Fatal(err)
			}
			before := read()
			if len(before) != 1 || before[0].Revision != 1 {
				t.Fatalf("same-scope handler was not saved: %+v", before)
			}
			for _, scope := range []protocol.DispatchScope{{}, {Worktree: &f.other}} {
				handler := before[0]
				handler.Callback.Scope = scope
				if err := save(handler, true); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
					t.Fatalf("scope-widening update was not refused: %v", err)
				}
				if after := read(); !reflect.DeepEqual(after, before) {
					t.Fatalf("rejected update changed stored handlers: before=%+v after=%+v", before, after)
				}
			}
			handler := before[0]
			handler.Enabled = false
			if err := save(handler, false); err != nil {
				t.Fatal(err)
			}
			if after := read(); len(after) != 1 || after[0].Revision != 2 || after[0].Enabled {
				t.Fatalf("same-scope update was not saved: %+v", after)
			}
		})
	}
	for _, scenario := range []struct {
		name     string
		nodeWide bool
		outside  bool
	}{
		{name: "saved_worktree_scope_reaches_execution"},
		{name: "event_binding_cannot_escape_saved_worktree", outside: true},
		{name: "node_wide_handler_can_choose_another_worktree", nodeWide: true, outside: true},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			t.Parallel()
			f := newDispatchScopeFixture(t)
			ctx := t.Context()
			if scenario.nodeWide {
				f.scope.Worktree = nil
				f.handler.Callback.Scope = protocol.DispatchScope{}
			}
			target, project := f.worktree, f.project
			if scenario.outside {
				target, project = f.other, f.otherProject
			}
			payload, err := json.Marshal(map[string]any{"worktree": target, "project": project})
			if err != nil {
				t.Fatal(err)
			}
			err = f.storage.Node.Write(ctx, func(db *sql.Conn) error {
				if err := f.dispatch(ctx, db, protocol.DispatchCommand{Kind: "save_handler", Data: &f.handler}); err != nil {
					return err
				}
				if err := f.dispatch(ctx, db, protocol.DispatchCommand{Kind: "enqueue", Data: protocol.DispatchCommandEnqueuePayload{Handler: f.handler.Name, Payload: payload}}); err != nil {
					return err
				}
				batch, _, err := DispatchTick(ctx, db, f.node, f.now.UnixMilli(), false)
				if err != nil {
					return err
				}
				if len(batch.Work) != 1 || batch.Work[0].Request.Plugin == nil {
					return fmt.Errorf("saved callback was not claimed: %+v", batch)
				}
				request := batch.Work[0].Request
				if !reflect.DeepEqual(request.Plugin.Worktree, f.scope.Worktree) || request.Plugin.Session != nil {
					return errors.New("claimed callback lost its saved scope")
				}
				info, err := DispatchPackage(ctx, db, f.node, f.node, request)
				if err != nil || info == nil {
					return fmt.Errorf("saved callback package was not recovered: %v", err)
				}
				if !scenario.nodeWide {
					forged := request
					forged.Plugin = new(*request.Plugin)
					forged.Plugin.Worktree = nil
					if _, err := DispatchPackage(ctx, db, f.node, f.node, forged); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
						return fmt.Errorf("claimed scope could be removed: %v", err)
					}
				}
				err = CheckPluginRequest(ctx, db, request, *info)
				if scenario.outside && !scenario.nodeWide {
					if !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
						return fmt.Errorf("out-of-scope event binding was admitted: %v", err)
					}
					var sessions int
					if err := db.QueryRowContext(ctx, "SELECT count(*) FROM sessions").Scan(&sessions); err != nil {
						return err
					}
					if sessions != 1 {
						return fmt.Errorf("rejected callback changed the session count: %d", sessions)
					}
					return nil
				}
				if err != nil {
					return err
				}
				body, err := encode(request)
				if err != nil {
					return err
				}
				if previous, err := admission.Admit(ctx, db, f.node, request, body); err != nil || previous != nil {
					return fmt.Errorf("unexpected callback admission: %v", err)
				}
				output, events, err := ExecuteAgentCommand(ctx, db, f.node, f.storage.Profile, f.node, request, f.now)
				if err != nil {
					return err
				}
				if _, err := RecordEvents(ctx, db, f.node, events); err != nil {
					return err
				}
				if err := admission.Complete(ctx, db, f.node, request.ID, link.Response{Output: output}); err != nil {
					return err
				}
				turn := output.Data.(protocol.QueuedTurn)
				session, err := ReadSession(ctx, db, turn.Session)
				if err != nil {
					return err
				}
				if session.Worktree != target {
					return errors.New("callback started a session in the wrong worktree")
				}
				owned, err := ReadDispatchTurn(ctx, db, f.node, f.scope.Package.Name, batch.Work[0].Job)
				if err == nil && owned != turn.ID {
					err = errors.New("saved callback does not own its admitted turn")
				}
				return err
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

type dispatchScopeFixture struct {
	storage               *Store
	node                  protocol.NodeID
	info                  protocol.PluginInfo
	scope                 protocol.PluginContext
	handler               protocol.DispatchHandler
	project, otherProject protocol.ProjectID
	worktree, other       protocol.WorktreeID
	now                   time.Time
}

func newDispatchScopeFixture(t *testing.T) dispatchScopeFixture {
	t.Helper()
	storage, node, base, _, now := agentHistoryFixture(t, protocol.WorkModeCode)
	f := dispatchScopeFixture{storage: storage, node: node, info: pluginTestInfo("scope-fixture", 'b'), now: now,
		project: protocol.NewProjectID(), otherProject: protocol.NewProjectID(), worktree: base.Worktree, other: protocol.NewWorktreeID()}
	f.info.Summary.Revision, f.info.Summary.Enabled = 1, true
	f.info.Extension = new(protocol.DefaultPluginExtension())
	f.info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionDispatch, protocol.PluginActionStartSessions}
	f.scope = protocol.DefaultPluginContext()
	f.scope.Package, f.scope.Worktree = dispatchReference(f.info.Summary), &f.worktree
	draft := protocol.DefaultConversationStart()
	draft.Project, draft.Worktree, draft.Config = &f.project, &f.worktree, &base.Config
	draft.Title, draft.Message.Text, draft.Execution = "Scoped task", "Run the scoped task", protocol.ConversationExecutionModel
	f.handler = protocol.DispatchHandler{Name: "scoped", Enabled: true, Queue: "scoped",
		Source: protocol.DispatchSource{Package: f.scope.Package.Name, Topic: "scoped"},
		Callback: protocol.DispatchCallback{Scope: protocol.DispatchScope{Worktree: &f.worktree}, Completion: protocol.DispatchCompletionTurn,
			Command: &protocol.Command{Kind: "start_session", Data: &draft}, Bindings: map[string]string{"/data/worktree": "/payload/worktree", "/data/project": "/payload/project"}}}
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		body, err := encode(f.info)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO plugins(name,revision,installed,body) VALUES(?1,1,1,?2)", f.scope.Package.Name, body); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO plugin_packages(name,digest,body) VALUES(?1,?2,?3)", f.scope.Package.Name, f.scope.Package.Digest, body); err != nil {
			return err
		}
		for _, project := range []protocol.ProjectID{f.project, f.otherProject} {
			if _, err := db.ExecContext(ctx, "INSERT INTO projects(id,name,path,appearance) VALUES(?1,'Fixture',?2,'{}')", project.String(), "/fixture-project/"+project.String()); err != nil {
				return err
			}
		}
		if _, err := db.ExecContext(ctx, "UPDATE worktrees SET project=?2 WHERE id=?1", f.worktree.String(), f.project.String()); err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,'/fixture-other',1)", f.other.String(), f.otherProject.String())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return f
}

func (f dispatchScopeFixture) dispatch(ctx context.Context, db *sql.Conn, action protocol.DispatchCommand) error {
	request := protocol.Request{Plugin: &f.scope, Command: protocol.Command{Kind: "dispatch", Data: &protocol.CommandDispatchPayload{Package: f.scope.Package, Action: action}}}
	if err := CheckPluginRequest(ctx, db, request, f.info); err != nil {
		return err
	}
	_, _, err := ExecuteDispatchAdmitted(ctx, db, f.node, f.scope.Package, action, f.now.UnixMilli())
	return err
}
