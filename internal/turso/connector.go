// Package turso configures the official Turso database/sql connector for Node storage.
package turso

import (
	"context"
	"database/sql/driver"
	"fmt"
	"strings"
	"sync"
)

// The system library is the pinned FTS-enabled SDK shipped with Sailry. The
// official SDK owns native loading, SQL execution and connection lifetimes.
var initialize = sync.OnceValue(func() (err error) {
	defer func() {
		if failure := recover(); failure != nil {
			err = fmt.Errorf("initialize Turso: %v", failure)
		}
	}()
	if err := initializeLibrary(); err != nil {
		return err
	}
	initializeBridge()
	return nil
})

// Connector applies Node's durability and isolation settings to every pooled
// connection. Turso shares the native database for the same canonical path.
func Connector(path string, readonly bool) (driver.Connector, error) {
	if strings.ContainsRune(path, 0) {
		return nil, fmt.Errorf("Turso database path contains an unsupported character")
	}
	if err := initialize(); err != nil {
		return nil, err
	}
	return &configuredConnector{path: path, readonly: readonly}, nil
}

type configuredConnector struct {
	path     string
	readonly bool
}

func (connector *configuredConnector) Driver() driver.Driver { return configuredDriver{} }

type configuredDriver struct{}

func (configuredDriver) Open(path string) (driver.Conn, error) {
	connector, err := Connector(path, false)
	if err != nil {
		return nil, err
	}
	return connector.Connect(context.Background())
}

func (connector *configuredConnector) Connect(ctx context.Context) (driver.Conn, error) {
	connection, err := openConnection(ctx, connector.path)
	if err != nil {
		return nil, err
	}
	pragmas := []string{"PRAGMA synchronous=FULL", "PRAGMA foreign_keys=1"}
	if connector.readonly {
		pragmas = append(pragmas, "PRAGMA query_only=1")
	}
	for _, pragma := range pragmas {
		if _, err := connection.ExecContext(ctx, pragma, nil); err != nil {
			_ = connection.Close()
			return nil, err
		}
	}
	return connection, nil
}
