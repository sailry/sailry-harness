package store

import (
	"context"
	"database/sql"
)

type SchemaObject struct {
	Kind string
	Name string
}

func ReadSchemaObjects(ctx context.Context, connection *sql.Conn) (map[SchemaObject]string, error) {
	objects, err := readSchemaObjects(ctx, connection)
	if err != nil {
		return nil, err
	}
	result := make(map[SchemaObject]string, len(objects))
	for identity, declaration := range objects {
		result[SchemaObject{Kind: identity.kind, Name: identity.name}] = declaration
	}
	return result, nil
}

func CheckDatabaseFiles(path string) error { return checkDatabaseFiles(path) }
