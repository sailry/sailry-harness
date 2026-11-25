package sdktest

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/native/storage"
)

type Store = store.Store

type AgentState = store.AgentState

type ApprovalBinding = store.ApprovalBinding

type ConnectionCatalog = store.ConnectionCatalog

type Counter = store.Counter

const (
	CounterSessions    = store.CounterSessions
	CounterTurns       = store.CounterTurns
	CounterAgentEvents = store.CounterAgentEvents
)

func OpenStore(ctx context.Context, profile string, node protocol.NodeID) (*Store, error) {
	return store.Open(ctx, profile, node)
}

func AcquireProfile(path string) (*core.Profile, error) { return core.AcquireProfile(path) }

func NextCounter(ctx context.Context, connection *sql.Conn, namespace Counter) (uint64, error) {
	return store.NextCounter(ctx, connection, namespace)
}

func ValidateCounters(ctx context.Context, connection *sql.Conn) error {
	return store.ValidateCountersForTesting(ctx, connection)
}

func ReplaceWriterConnection(ctx context.Context, handle *storage.Handle, connection *sql.Conn) (func() error, error) {
	return store.ReplaceWriterConnectionForTesting(ctx, handle, connection)
}

func ReadConversation(ctx context.Context, db *sql.Conn, session protocol.SessionID, before *protocol.TurnID, limit uint16) (protocol.ConversationHistory, error) {
	return store.ReadConversation(ctx, db, session, before, limit)
}

func ReadAgentRun(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationRun, error) {
	return store.ReadAgentRun(ctx, db, turn)
}

func ReadSession(ctx context.Context, db *sql.Conn, session protocol.SessionID) (protocol.Session, error) {
	return store.ReadSession(ctx, db, session)
}

func ReadConnectionCatalog(ctx context.Context, db *sql.Conn, session protocol.SessionID) (ConnectionCatalog, error) {
	return store.ReadConnectionCatalog(ctx, db, session)
}

func ReadDatabase(ctx context.Context, db *sql.Conn, id protocol.DatabaseID) (*protocol.DatabaseProfile, error) {
	return store.ReadDatabase(ctx, db, id)
}

func ReadSsh(ctx context.Context, db *sql.Conn, id protocol.SshID, revision uint64) (protocol.SshProfile, protocol.SshCredential, error) {
	return store.ReadSsh(ctx, db, id, revision)
}

func CurrentUser(ctx context.Context, db *sql.Conn) (protocol.User, error) {
	return store.CurrentUser(ctx, db)
}

func LoadAgentState(ctx context.Context, db *sql.Conn, session protocol.SessionID) (AgentState, error) {
	return store.LoadAgentState(ctx, db, session)
}

func ReadPlugin(ctx context.Context, db *sql.Conn, name string) (*protocol.PluginInfo, error) {
	return store.ReadPlugin(ctx, db, name)
}

func InstallPlugin(ctx context.Context, db *sql.Conn, info protocol.PluginInfo, revision uint64, enabled bool) (protocol.PluginInfo, error) {
	return store.InstallPlugin(ctx, db, info, revision, enabled)
}

func PluginConnectionAction(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.PluginAction, bool, error) {
	return store.PluginConnectionAction(ctx, db, request)
}

func ConnectionCreated(ctx context.Context, db *sql.Conn, turn protocol.TurnID, owner protocol.PluginReference, target protocol.ConnectionResource) (bool, error) {
	return store.ConnectionCreated(ctx, db, turn, owner, target)
}

func SetLinkTrust(ctx context.Context, db *sql.Conn, caller protocol.NodeID, trusted bool) error {
	return store.SetLinkTrust(ctx, db, caller, trusted)
}

func NativeMemoryReference() protocol.PluginReference { return store.NativeMemoryReference() }
