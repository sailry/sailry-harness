package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) connectorConnections(ctx context.Context) ([]protocol.PluginConnectorConnection, error) {
	var values []protocol.PluginConnectorConnection
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		values, err = store.Connections(ctx, db)
		return err
	})
	if err != nil {
		return nil, err
	}
	for index, value := range values {
		var previous store.ConnectorRecord
		var capture store.ConnectorLoginCapture
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			previous, err = store.ReadConnectorRecord(ctx, db, value.ID)
			if err == nil {
				capture, err = store.PrepareConnectorConsent(ctx, db, value.ID, value.Revision)
			}
			return err
		})
		if err != nil {
			continue
		}
		if err := node.captureConnectorTargets(ctx, &capture); err != nil {
			if previous.Bound != nil {
				values[index] = store.ProjectConnectorConsent(value, previous.Bound, nil)
			}
			continue
		}
		values[index] = store.ProjectConnectorConsent(value, previous.Bound, capture.Record.Bound)
	}
	return values, nil
}
