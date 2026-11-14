package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchSessionStartCapture(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"project", "unassigned", "source_removed", "source_lowered", "legacy_handler", "legacy_job"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			f := newDispatchScopeFixture(t)
			ctx := t.Context()
			err := f.storage.Node.Write(ctx, func(db *sql.Conn) error {
				source, parent, err := dispatchSourceSession(ctx, db, &f, scenario != "unassigned")
				if err != nil {
					return err
				}
				source.Config.Permission = protocol.PermissionFull
				source.Config.Options = map[string]map[string]json.RawMessage{"another-package": {"enabled": json.RawMessage("true")}}
				output, _, err := reviseSession(ctx, db, source)
				if err != nil {
					return err
				}
				source = output.Data.(protocol.Session)
				draft := *f.handler.Callback.Command.Data.(*protocol.ConversationStart)
				draft.Config = &source.Config
				f.handler.Callback.Command = &protocol.Command{Kind: "start_session", Data: &draft}
				if err := f.dispatch(ctx, db, protocol.DispatchCommand{Kind: "save_handler", Data: &f.handler}); err != nil {
					return err
				}
				// An ordinary caller cannot turn the same scope and draft into
				// a new session by claiming to be a scheduled invocation.
				ordinary := protocol.NewRequest(f.node, *f.handler.Callback.Command)
				ordinary.Plugin = &f.scope
				if _, err := AuthorizePluginRequest(ctx, db, f.node, f.node, ordinary, nil); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
					return fmt.Errorf("ordinary session caller created another session: %v", err)
				}
				if scenario == "legacy_handler" {
					if _, err := db.ExecContext(ctx, "UPDATE dispatch_handlers SET reference=json_remove(reference,'$.session_start')"); err != nil {
						return err
					}
				}
				if err := f.dispatch(ctx, db, protocol.DispatchCommand{Kind: "enqueue", Data: protocol.DispatchCommandEnqueuePayload{Handler: f.handler.Name, Payload: []byte("null")}}); err != nil {
					return err
				}
				if scenario == "legacy_job" {
					if _, err := db.ExecContext(ctx, "UPDATE dispatch_jobs SET reference=json_remove(reference,'$.session_start')"); err != nil {
						return err
					}
				}
				if scenario == "source_removed" {
					if _, _, err := FinishTurn(ctx, db, parent, protocol.ConversationStatusCompleted, nil, f.now); err != nil {
						return err
					}
					if _, _, err := removeSession(ctx, db, source.ID, source.Revision); err != nil {
						return err
					}
				} else if scenario == "source_lowered" {
					source.Config.Permission, source.Config.Mode = protocol.PermissionAsk, protocol.WorkModePlan
					if _, _, err := reviseSession(ctx, db, source); err != nil {
						return err
					}
				}
				batch, _, err := DispatchTick(ctx, db, f.node, f.now.UnixMilli(), true)
				if err != nil || len(batch.Work) != 1 {
					return fmt.Errorf("callback was not recovered: %d, %v", len(batch.Work), err)
				}
				request := batch.Work[0].Request
				if request.Plugin == nil || request.Plugin.Session == nil || *request.Plugin.Session != source.ID || request.Plugin.Worktree == nil || *request.Plugin.Worktree != source.Worktree {
					return errors.New("callback lost its source identity")
				}
				if _, err := AuthorizePluginRequest(ctx, db, f.node, protocol.NodeID{91}, request, nil); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
					return fmt.Errorf("remote caller borrowed local callback proof: %v", err)
				}
				_, err = AuthorizePluginRequest(ctx, db, f.node, f.node, request, nil)
				if scenario == "legacy_handler" || scenario == "legacy_job" {
					if !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
						return fmt.Errorf("legacy unvalidated callback was enabled: %v", err)
					}
					return nil
				}
				if err != nil {
					return err
				}
				// Package collection decodes the same private reference envelope.
				if _, err := CollectPlugins(ctx, db); err != nil {
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
				created, err := ReadSession(ctx, db, turn.Session)
				if err != nil {
					return err
				}
				if created.ID == source.ID || turn.Config.Permission != protocol.PermissionFull || turn.Config.Mode != protocol.WorkModeCode || !reflect.DeepEqual(turn.Config.Options, draft.Config.Options) {
					return errors.New("callback did not retain its saved configuration")
				}
				if scenario == "unassigned" {
					if created.Project != nil || created.Worktree == source.Worktree {
						return errors.New("unassigned callback did not create a fresh workspace")
					}
				} else if created.Worktree != f.worktree || created.Project == nil || *created.Project != f.project {
					return errors.New("callback escaped its saved project or worktree")
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestScheduledSessionCannotRebindAuthority(t *testing.T) {
	t.Parallel()
	f := newDispatchScopeFixture(t)
	ctx := t.Context()
	err := f.storage.Node.Write(ctx, func(db *sql.Conn) error {
		source, parent, err := dispatchSourceSession(ctx, db, &f, true)
		if err != nil {
			return err
		}
		f.scope.Turn = &parent
		// The running turn still has Ask even after the controller selects Full.
		source.Config.Permission = protocol.PermissionFull
		if _, _, err := reviseSession(ctx, db, source); err != nil {
			return err
		}
		for _, change := range []string{"config", "default_config", "worktree", "project", "foreign_options", "/data", "/data/config", "/data/config/permission", "/data/worktree", "/data/project", "/"} {
			handler := f.handler
			draft := *handler.Callback.Command.Data.(*protocol.ConversationStart)
			config := *draft.Config
			draft.Config = &config
			handler.Callback.Command = &protocol.Command{Kind: "start_session", Data: &draft}
			switch change {
			case "config":
				config.Permission = protocol.PermissionFull
			case "default_config":
				draft.Config = nil
			case "worktree":
				draft.Worktree = &f.other
			case "project":
				draft.Project = &f.otherProject
			case "foreign_options":
				config.Options = map[string]map[string]json.RawMessage{"another-package": {"enabled": json.RawMessage("true")}}
			default:
				handler.Callback.Bindings = map[string]string{change: "/payload"}
			}
			code := protocol.ErrorCodePermissionDenied
			if change == "/" {
				code = protocol.ErrorCodeInvalidRequest
			}
			if err := f.dispatch(ctx, db, protocol.DispatchCommand{Kind: "save_handler", Data: &handler}); !providerFixtureFault(err, code) {
				return fmt.Errorf("callback rebound %s: %v", change, err)
			}
		}
		return f.dispatch(ctx, db, protocol.DispatchCommand{Kind: "save_handler", Data: &f.handler})
	})
	if err != nil {
		t.Fatal(err)
	}
}

func dispatchSourceSession(ctx context.Context, db *sql.Conn, f *dispatchScopeFixture, project bool) (protocol.Session, protocol.TurnID, error) {
	sessions, err := ReadSessions(ctx, db)
	if err != nil || len(sessions) != 1 {
		return protocol.Session{}, protocol.TurnID{}, fmt.Errorf("fixture source session: %d, %v", len(sessions), err)
	}
	source := sessions[0]
	var turn string
	if err := db.QueryRowContext(ctx, "SELECT id FROM turns WHERE session=?1 ORDER BY rowid LIMIT 1", source.ID.String()).Scan(&turn); err != nil {
		return source, protocol.TurnID{}, err
	}
	parent, err := protocol.ParseTurnID(turn)
	if err != nil {
		return source, parent, err
	}
	f.scope.Session = &source.ID
	f.handler.Callback.Scope.Session = &source.ID
	f.handler.Callback.Bindings = map[string]string{}
	draft := f.handler.Callback.Command.Data.(*protocol.ConversationStart)
	if project {
		source.Project = &f.project
		_, err = db.ExecContext(ctx, "UPDATE sessions SET project=?2 WHERE id=?1", source.ID.String(), f.project.String())
	} else {
		draft.Project, draft.Worktree = nil, nil
		_, err = db.ExecContext(ctx, "UPDATE worktrees SET project=NULL WHERE id=?1", source.Worktree.String())
	}
	return source, parent, err
}
