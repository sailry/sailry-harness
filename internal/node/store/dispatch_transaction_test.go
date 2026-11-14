package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDispatchTransactionAuthorizationOrder(t *testing.T) {
	t.Parallel()
	for _, scenario := range []string{"handler_then_schedule", "schedule_before_handler", "missing_handler_rolls_back"} {
		t.Run(scenario, func(t *testing.T) {
			t.Parallel()
			f := newDispatchScopeFixture(t)
			ctx := t.Context()
			handler := protocol.PluginTransactionOperation{Kind: "dispatch", Data: protocol.DispatchCommand{Kind: "save_handler", Data: &f.handler}}
			schedule := protocol.PluginTransactionOperation{Kind: "dispatch", Data: protocol.DispatchCommand{Kind: "save_schedule", Data: protocol.DispatchSchedule{
				ID: protocol.NewScheduleID(), Enabled: true, Topic: f.handler.Source.Topic, Payload: []byte("null"),
				Timing: protocol.DispatchTiming{Kind: "once", Data: protocol.DispatchTimingOncePayload{AtMs: f.now.UnixMilli() + 1000}},
			}}}
			operations := []protocol.PluginTransactionOperation{handler, schedule}
			switch scenario {
			case "schedule_before_handler":
				operations = []protocol.PluginTransactionOperation{schedule, handler}
			case "missing_handler_rolls_back":
				other := f.handler
				other.Name, other.Source.Topic = "other", "other"
				operations[0] = protocol.PluginTransactionOperation{Kind: "dispatch", Data: protocol.DispatchCommand{Kind: "save_handler", Data: &other}}
			}
			request := protocol.NewRequest(f.node, protocol.Command{Kind: "plugin_transaction", Data: &protocol.CommandPluginTransactionPayload{Operations: operations}})
			request.Plugin = &f.scope
			// OAuth preparation cannot see writes that the transaction has not
			// committed. It must leave their dependent checks to the writer.
			if err := f.storage.Node.Read(ctx, func(db *sql.Conn) error {
				providers, err := PluginTransactionProviders(ctx, db, request)
				if err != nil {
					return err
				}
				if len(providers) != 0 {
					return fmt.Errorf("dispatch-only transaction refreshed providers: %v", providers)
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
			err := f.storage.Node.Write(ctx, func(db *sql.Conn) error {
				if err := CheckPluginRequest(ctx, db, request, f.info); err != nil {
					return err
				}
				_, _, err := ExecutePluginStorageCommand(ctx, db, f.node, f.storage.Profile, f.node, request)
				return err
			})
			allowed := scenario == "handler_then_schedule"
			if allowed && err != nil || !allowed && !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
				t.Fatalf("transaction allowed=%t: %v", allowed, err)
			}
			if err := f.storage.Node.Read(ctx, func(db *sql.Conn) error {
				for _, table := range []string{"dispatch_handlers", "dispatch_schedules"} {
					var count int
					if err := db.QueryRowContext(ctx, "SELECT count(*) FROM "+table).Scan(&count); err != nil {
						return err
					}
					want := 0
					if allowed {
						want = 1
					}
					if count != want {
						return fmt.Errorf("%s contains %d rows, want %d", table, count, want)
					}
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}
