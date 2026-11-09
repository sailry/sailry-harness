package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type Counter string

const (
	CounterEvents         Counter = "events"
	CounterPluginRecords  Counter = "plugin_conversation_records"
	CounterAgentEvents    Counter = "agent_events"
	CounterDispatchEvents Counter = "dispatch_events"
	CounterDispatchJobs   Counter = "dispatch_jobs"
	CounterNotifications  Counter = "notifications"
	CounterSessions       Counter = "sessions"
	CounterTurns          Counter = "turns"
	CounterCheckpoints    Counter = "file_checkpoints"
	CounterArtifacts      Counter = "artifacts"
	CounterBackground     Counter = "agent_background"
)

// NextCounter advances a watermark inside the caller's existing write transaction.
func NextCounter(ctx context.Context, connection *sql.Conn, namespace Counter) (uint64, error) {
	var value int64
	err := connection.QueryRowContext(ctx,
		"INSERT INTO counters(namespace,value) VALUES(?1,1) ON CONFLICT(namespace) DO UPDATE SET value=counters.value+1 WHERE counters.value<9223372036854775807 RETURNING value",
		string(namespace)).Scan(&value)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, errors.New("sequence watermark is exhausted")
	}
	if err != nil {
		return 0, err
	}
	if value < 1 {
		return 0, errors.New("invalid sequence watermark")
	}
	return uint64(value), nil
}

// PutBlob stores binary content once per digest in the current transaction.
func PutBlob(ctx context.Context, connection *sql.Conn, data []byte) (string, error) {
	digest := blake3.Sum256(data)
	key := hex.EncodeToString(digest[:])
	if data == nil {
		data = []byte{}
	}
	_, err := connection.ExecContext(ctx, "INSERT INTO agent_blobs(digest,bytes) VALUES(?1,?2) ON CONFLICT(digest) DO NOTHING", key, data)
	return key, err
}

func GetBlob(ctx context.Context, connection *sql.Conn, digest string) ([]byte, error) {
	var data []byte
	err := connection.QueryRowContext(ctx, "SELECT bytes FROM agent_blobs WHERE digest=?1", digest).Scan(&data)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "conversation asset is unavailable"}
	}
	return data, err
}
