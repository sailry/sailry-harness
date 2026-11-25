package store

import (
	"context"
	"database/sql"
)

func ValidateCountersForTesting(ctx context.Context, connection *sql.Conn) error {
	return validateCounters(ctx, connection)
}

func ReplaceWriterConnectionForTesting(ctx context.Context, handle *Handle, connection *sql.Conn) (func() error, error) {
	exclusive, err := handle.Exclusive(ctx)
	if err != nil {
		return nil, err
	}
	handle.mu.Lock()
	writer := handle.state.writer
	original := writer.connection
	writer.connection = connection
	handle.mu.Unlock()
	if err := exclusive.Close(); err != nil {
		return nil, err
	}
	return func() error {
		exclusive, err := handle.Exclusive(context.Background())
		if err != nil {
			return err
		}
		handle.mu.Lock()
		writer.connection = original
		handle.mu.Unlock()
		return exclusive.Close()
	}, nil
}
