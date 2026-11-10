package node

import (
	"context"
	"database/sql"
	"log"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/power"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// initPower creates the owner of the system's idle-sleep request. It holds
// the request while the saved setting is on and a turn, command, supervised
// task, knowledge indexing, code query or backup runs.
func (node *Node) initPower(ctx context.Context, backend power.Backend) error {
	// A test binary never takes the machine's real request.
	if backend == nil && testing.Testing() {
		backend = power.Unsupported()
	}
	if backend == nil {
		node.power = power.New(node.powerFailed)
	} else {
		node.power = power.WithBackend(backend, node.powerFailed)
	}
	return node.applyPower(ctx)
}

// applyPower applies current storage: a replayed durable result may carry an
// old saved value.
func (node *Node) applyPower(ctx context.Context) error {
	var settings protocol.PowerSettings
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		settings, err = store.ReadPower(ctx, db)
		return err
	})
	if err == nil {
		node.power.Apply(settings)
	}
	return err
}

// powerFailed records the system's first refusal as an error notice.
func (node *Node) powerFailed(code int64) {
	log.Printf("native idle-sleep request failed: %d", code)
	err := node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
		changes, err := store.PowerFailed(node.ctx, db, code)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(node.ctx, db, node.ID(), changes)
		return err
	})
	if err != nil {
		log.Printf("power failure notification persistence failed: %v", err)
		return
	}
	node.publish()
}

// systemEvents applies committed power and telemetry settings to the process.
func (node *Node) systemEvents(changes []protocol.Event) {
	for _, change := range changes {
		switch change.Kind {
		case "power_settings_changed":
			if err := node.applyPower(node.ctx); err != nil {
				log.Printf("power settings could not be applied: %v", err)
			}
		case "telemetry_settings_changed":
			if err := node.applyTelemetry(node.ctx); err != nil {
				log.Printf("telemetry settings could not be applied: %v", err)
			}
		}
	}
}
