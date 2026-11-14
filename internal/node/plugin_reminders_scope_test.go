package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestReminderCallbackOutlivesSourceSession(t *testing.T) {
	// Keep dispatch before the reminder's due time. The callback below runs the
	// bundled JavaScript directly, after its originating session is removed.
	clock := clockwork.NewFakeClockAt(time.Unix(0, 0))
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{
		DispatchClock: clock, Environment: &environment, MCP: fixtureMCPOptions(),
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { node.Stop(context.Background()) })
	ctx := t.Context()
	source := sessionFixture(t, node)
	scope := protocol.DefaultPluginContext()
	scope.Worktree, scope.Session = &source.Worktree, &source.ID
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		info, err := store.ReadPlugin(ctx, db, "reminders")
		if err != nil {
			return err
		}
		if info == nil || !info.Summary.Enabled {
			return errors.New("the bundled reminders package is not installed and enabled")
		}
		scope.Package = plugins.PackageReference(*info)
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	call := func(handler string, input any) json.RawMessage {
		t.Helper()
		body, err := json.Marshal(input)
		if err != nil {
			t.Fatal(err)
		}
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "call_plugin", Data: protocol.CommandCallPluginPayload{Handler: handler, Input: body}})
		request.Plugin = &scope
		response := dispatchFixture(t, node, node.ID(), request)
		if response.Fault != nil {
			t.Fatalf("%s: %v", handler, response.Fault)
		}
		body, err = json.Marshal(response.Output.Data)
		if err != nil {
			t.Fatal(err)
		}
		return body
	}
	id := protocol.NewRequestID().String()
	due := time.Now().Add(-time.Second).UnixMilli()
	const title, message = "Scoped reminder fixture", "Persisted after source removal"
	raw := call("save", map[string]any{
		"id": id, "revision": "0", "project": nil, "title": title, "message": message,
		"completed": false, "due_ms": due,
	})
	var saved struct {
		Ok *struct {
			Revision string `json:"revision"`
		} `json:"Ok"`
		Err *protocol.Fault `json:"Err"`
	}
	if err := json.Unmarshal(raw, &saved); err != nil || saved.Err != nil || saved.Ok == nil || saved.Ok.Revision != "1" {
		t.Fatalf("save: %s (%v)", raw, err)
	}
	var handler protocol.DispatchHandler
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var body []byte
		if err := db.QueryRowContext(ctx, "SELECT body FROM dispatch_handlers WHERE package=?1 AND name=?2", scope.Package.Name, id).Scan(&body); err != nil {
			return err
		}
		return json.Unmarshal(body, &handler)
	}); err != nil {
		t.Fatal(err)
	}
	if handler.Callback.Scope.Worktree == nil || *handler.Callback.Scope.Worktree != source.Worktree ||
		handler.Callback.Scope.Session == nil || *handler.Callback.Scope.Session != source.ID {
		t.Fatalf("callback lost its source scope: %+v", handler.Callback.Scope)
	}
	commandFixture(t, node, protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{
		Session: source.ID, ExpectedRevision: source.Revision,
	}})
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		_, err := store.ReadSession(ctx, db, source.ID)
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeNotFound {
			return errors.New("the source session must be removed before the callback runs")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	for _, delivery := range []string{"delivered", "skipped"} {
		raw = call("notify", map[string]any{"id": id, "revision": "1"})
		var result map[string]bool
		if err := json.Unmarshal(raw, &result); err != nil || len(result) != 1 || !result[delivery] {
			t.Fatalf("notify expected %s: %s (%v)", delivery, raw, err)
		}
		var entry protocol.PluginStorageEntry
		var notices []protocol.NotificationNotice
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			entry, err = store.ReadPluginValue(ctx, db, scope.Package, "reminder/"+id)
			if err != nil {
				return err
			}
			notices, err = store.ReadNotifications(ctx, db)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		var item struct {
			Title      string `json:"title"`
			Message    string `json:"message"`
			NotifiedMs *int64 `json:"notified_ms"`
		}
		if err := json.Unmarshal(entry.Value, &item); err != nil || !entry.Present || entry.Revision != 2 ||
			item.Title != title || item.Message != message || item.NotifiedMs == nil || *item.NotifiedMs < due {
			t.Fatalf("stored reminder after %s: %+v (%v)", delivery, entry, err)
		}
		if len(notices) != 1 || notices[0].Package != scope.Package.Name || notices[0].Content.Title != title ||
			notices[0].Content.Message != message || notices[0].Content.Session != nil {
			t.Fatalf("local notifications after %s: %+v", delivery, notices)
		}
	}
}
