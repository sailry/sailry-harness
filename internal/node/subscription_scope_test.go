package node

import (
	"context"
	"database/sql"
	"errors"
	"io"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCachedNodeSnapshotUsesCurrentProjectScope(t *testing.T) {
	for _, change := range []string{"ordinary write", "project revoked", "controller revoked"} {
		t.Run(change, func(t *testing.T) {
			t.Parallel()
			node := nodeFixture(t)
			ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
			defer cancel()
			caller := protocol.NodeID{41}
			project, worktree := subscriptionProjectFixture(t, ctx, node, caller)
			info := protocol.DefaultTerminalInfo()
			info.ID, info.Worktree, info.Revision = protocol.NewTerminalID(), &worktree, 1
			info.Status = protocol.TerminalStatus{Kind: "running"}
			var start uint64
			subscriptionWrite(t, ctx, node, func(db *sql.Conn) error {
				var err error
				start, err = recordSubscriptionTerminal(ctx, db, node.ID(), info)
				return err
			})
			stream, err := node.Subscribe(ctx, caller, protocol.Topic{Kind: "node"})
			if err != nil {
				t.Fatal(err)
			}
			defer stream.Close()

			// The initial snapshot is pinned. The following commits deliberately
			// send no wake hint, and no data has yet been delivered to the caller.
			var changed, revoked uint64
			info.Revision++
			subscriptionWrite(t, ctx, node, func(db *sql.Conn) error {
				var err error
				changed, err = recordSubscriptionTerminal(ctx, db, node.ID(), info)
				return err
			})
			switch change {
			case "project revoked":
				subscriptionWrite(t, ctx, node, func(db *sql.Conn) error {
					if err := privatizeSubscriptionProject(ctx, db, project); err != nil {
						return err
					}
					events, err := store.RecordEvents(ctx, db, node.ID(), []protocol.Event{{Kind: "projection_invalidated"}})
					if err == nil {
						revoked = events[0].Cursor
					}
					return err
				})
			case "controller revoked":
				subscriptionWrite(t, ctx, node, func(db *sql.Conn) error { return store.SetLinkTrust(ctx, db, caller, false) })
			}
			update, err := stream.Next(ctx)
			if change == "controller revoked" {
				if !pluginMessageFault(err, protocol.ErrorCodePermissionDenied) || update.Data != nil {
					t.Fatalf("revoked controller received a snapshot: %s, %v", update.Kind, err)
				}
				return
			}
			snapshot, ok := update.Data.(protocol.Snapshot)
			if err != nil || update.Kind != "snapshot" || !ok || snapshot.User == nil || snapshot.User.Local {
				t.Fatalf("initial update = %s, %T, %v", update.Kind, update.Data, err)
			}
			cursor, kind := changed, "terminal_changed"
			if change == "project revoked" {
				if snapshot.Cursor != revoked || len(snapshot.Projects) != 0 || len(snapshot.Worktrees) != 0 || len(snapshot.Terminals) != 0 {
					t.Fatalf("refreshed snapshot: cursor %d (want %d), projects %d, worktrees %d, terminals %d", snapshot.Cursor, revoked, len(snapshot.Projects), len(snapshot.Worktrees), len(snapshot.Terminals))
				}
				info.Revision++
				subscriptionWrite(t, ctx, node, func(db *sql.Conn) error {
					var err error
					cursor, err = recordSubscriptionTerminal(ctx, db, node.ID(), info)
					return err
				})
				kind = "cursor"
			} else if snapshot.Cursor != start || len(snapshot.Projects) != 1 || len(snapshot.Worktrees) != 1 || len(snapshot.Terminals) != 1 || snapshot.Terminals[0].Revision != 1 {
				t.Fatalf("ordinary write replaced the pinned snapshot at cursor %d (want %d)", snapshot.Cursor, start)
			}
			update, err = stream.Next(ctx)
			event, ok := update.Data.(protocol.EventEnvelope)
			if err != nil || update.Kind != "event" || !ok || event.Cursor != cursor || event.Event.Kind != kind || kind == "cursor" && event.Event.Data != nil {
				t.Fatalf("first delta = %+v, %v; want %s at %d", update, err, kind, cursor)
			}
		})
	}
}

func TestAttachedRemoteTerminalDrainsOnlyWithCurrentAccess(t *testing.T) {
	for _, change := range []string{"shared project exits", "private terminal exits", "project revoked", "controller revoked"} {
		t.Run(change, func(t *testing.T) {
			t.Parallel()
			node := nodeFixture(t)
			ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
			defer cancel()
			caller := protocol.NodeID{42}
			project, worktree := subscriptionProjectFixture(t, ctx, node, caller)
			info := protocol.DefaultTerminalInfo()
			info.ID, info.Revision = protocol.NewTerminalID(), 1
			info.Status = protocol.TerminalStatus{Kind: "running"}
			if change == "private terminal exits" {
				profile := protocol.NewSshID()
				info.Ssh = &profile
			} else {
				info.Worktree = &worktree
			}
			subscriptionWrite(t, ctx, node, func(db *sql.Conn) error {
				user, err := store.CallerUser(ctx, db, node.ID(), caller)
				if err != nil {
					return err
				}
				_, err = recordSubscriptionTerminal(store.WithUser(ctx, user), db, node.ID(), info)
				return err
			})
			topic := protocol.Topic{Kind: "terminal", Data: info.ID}
			verified, err := node.authorizeTopic(ctx, caller, topic)
			if err != nil || userFromContext(verified).Local {
				t.Fatalf("remote admission = %v", err)
			}

			// Gate a scripted attached screen while inventory and permissions
			// change. No PTY, shell, SSH server, or timing-dependent poll is used.
			reading, release := make(chan struct{}), make(chan struct{})
			final := info
			final.Status = protocol.TerminalStatus{Kind: "exited", Data: protocol.TerminalStatusExitedPayload{Code: 0}}
			final.Revision++
			screen := protocol.DefaultTerminalScreen()
			screen.Columns = 80
			screen.VT = &protocol.TerminalVT{Viewport: protocol.TerminalViewport{Columns: 80, Rows: 24}, Bytes: protocol.Bytes("last bytes")}
			frame := protocol.TerminalFrame{Node: node.ID(), Info: final, Sequence: 1, Screen: protocol.TerminalScreenScreenUpdate{
				Kind: "replace", Data: protocol.TerminalScreenScreenUpdateReplacePayload{Screen: screen},
			}}
			calls := 0
			source := deliverySource(func(next context.Context) (protocol.Update, error) {
				calls++
				switch calls {
				case 1:
					initial := protocol.TerminalSnapshot{Node: node.ID(), Info: info, Screen: screen}
					initial.Screen.VT = &protocol.TerminalVT{Viewport: screen.VT.Viewport}
					return protocol.Update{Kind: "terminal_snapshot", Data: initial}, nil
				case 2:
					close(reading)
				default:
					return protocol.Update{}, io.EOF
				}
				select {
				case <-release:
					return protocol.Update{Kind: "terminal_frame", Data: frame}, nil
				case <-next.Done():
					return protocol.Update{}, next.Err()
				}
			})
			stream := node.trackSubscription(verified, caller, topic, source)
			defer stream.Close()
			if update, err := stream.Next(ctx); err != nil || update.Kind != "terminal_snapshot" {
				t.Fatalf("initial screen = %s, %v", update.Kind, err)
			}
			type result struct {
				update protocol.Update
				err    error
			}
			delivered := make(chan result, 1)
			go func() { update, err := stream.Next(ctx); delivered <- result{update, err} }()
			select {
			case <-reading:
			case early := <-delivered:
				t.Fatalf("terminal subscription ended before reading: %v", early.err)
			case <-ctx.Done():
				t.Fatal(ctx.Err())
			}
			node.terminalChangedBy(verified, final)
			if stream, err := node.Subscribe(ctx, caller, topic); !pluginMessageFault(err, protocol.ErrorCodePermissionDenied) {
				if stream != nil {
					_ = stream.Close()
				}
				t.Fatalf("finished terminal allowed a new subscription: %v", err)
			}
			switch change {
			case "project revoked":
				subscriptionWrite(t, ctx, node, func(db *sql.Conn) error { return privatizeSubscriptionProject(ctx, db, project) })
			case "controller revoked":
				subscriptionWrite(t, ctx, node, func(db *sql.Conn) error { return store.SetLinkTrust(ctx, db, caller, false) })
			}
			close(release)
			observed := <-delivered
			if change == "project revoked" || change == "controller revoked" {
				if !pluginMessageFault(observed.err, protocol.ErrorCodePermissionDenied) || observed.update.Data != nil {
					t.Fatalf("revoked caller received a final frame: %s, %v", observed.update.Kind, observed.err)
				}
				return
			}
			received, ok := observed.update.Data.(protocol.TerminalFrame)
			if observed.err != nil || observed.update.Kind != "terminal_frame" || !ok || received.Info.Status.Kind != "exited" {
				t.Fatalf("authorized final frame = %s, %T, %v", observed.update.Kind, observed.update.Data, observed.err)
			}
			replacement := received.Screen.Data.(protocol.TerminalScreenScreenUpdateReplacePayload)
			if replacement.Screen.VT == nil || string(replacement.Screen.VT.Bytes) != "last bytes" {
				t.Fatalf("final screen = %+v", replacement.Screen)
			}
			if update, err := stream.Next(ctx); !errors.Is(err, io.EOF) || update.Data != nil {
				t.Fatalf("drained subscription = %s, %v", update.Kind, err)
			}
		})
	}
}

func subscriptionProjectFixture(t *testing.T, ctx context.Context, node *Node, caller protocol.NodeID) (protocol.ProjectID, protocol.WorktreeID) {
	t.Helper()
	project, worktree := protocol.NewProjectID(), protocol.NewWorktreeID()
	subscriptionWrite(t, ctx, node, func(db *sql.Conn) error {
		if err := store.PairLinkPeer(ctx, db, caller, link.DirectionController); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, `INSERT INTO projects(id,name,path,appearance,visibility) VALUES(?1,'Shared','/subscription-fixture','{"icon":"folder","color":"none"}','shared')`, project.String()); err != nil {
			return err
		}
		_, err := db.ExecContext(ctx, `INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,'/subscription-fixture',1)`, worktree.String(), project.String())
		return err
	})
	return project, worktree
}

func subscriptionWrite(t *testing.T, ctx context.Context, node *Node, write func(*sql.Conn) error) {
	t.Helper()
	if err := node.profile.Store.Node.Write(ctx, write); err != nil {
		t.Fatal(err)
	}
}

func privatizeSubscriptionProject(ctx context.Context, db *sql.Conn, project protocol.ProjectID) error {
	_, err := db.ExecContext(ctx, "UPDATE projects SET visibility='private' WHERE id=?1", project.String())
	return err
}

func recordSubscriptionTerminal(ctx context.Context, db *sql.Conn, node protocol.NodeID, info protocol.TerminalInfo) (uint64, error) {
	changes, err := store.SaveTerminal(ctx, db, info, false)
	if err != nil {
		return 0, err
	}
	events, err := store.RecordEvents(ctx, db, node, changes)
	if err != nil {
		return 0, err
	}
	if len(events) != 1 {
		return 0, errors.New("fixture expected one terminal inventory event")
	}
	return events[0].Cursor, nil
}
