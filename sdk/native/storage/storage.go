package storage

import (
	"context"
	"database/sql"
	"database/sql/driver"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/turso"
)

type Handle = store.Handle

type SchemaObject = store.SchemaObject

func OpenHandle(ctx context.Context, path string) (*Handle, error) {
	return store.OpenHandle(ctx, path)
}

func ReadSchemaObjects(ctx context.Context, connection *sql.Conn) (map[SchemaObject]string, error) {
	return store.ReadSchemaObjects(ctx, connection)
}

func CheckDatabaseFiles(path string) error { return store.CheckDatabaseFiles(path) }

func Connector(path string, readonly bool) (driver.Connector, error) {
	return turso.Connector(path, readonly)
}
