package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func readPluginConversationValue(ctx context.Context, db *sql.Conn, name string, session protocol.SessionID, key string) (protocol.PluginStorageConversationEntry, error) {
	entry := protocol.DefaultPluginStorageConversationEntry()
	entry.Key = key
	if err := pluginKey(key, false); err != nil {
		return entry, err
	}
	var value []byte
	err := db.QueryRowContext(ctx, `SELECT v.revision,v.restored,r.value FROM plugin_conversation_values v
LEFT JOIN plugin_conversation_records r ON r.sequence=v.record WHERE v.name=?1 AND v.session=?2 AND v.key=?3`,
		name, session.String(), key).Scan(&entry.Revision, &entry.Restored, &value)
	if errors.Is(err, sql.ErrNoRows) {
		return entry, nil
	}
	if err != nil {
		return entry, err
	}
	if entry.Revision == 0 || value != nil && !json.Valid(value) {
		return entry, invalid("invalid stored plugin conversation value")
	}
	if value != nil {
		entry.Value, entry.Present = value, true
	}
	return entry, nil
}

func ReadPluginConversationValue(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, session protocol.SessionID, key string) (protocol.PluginStorageConversationEntry, error) {
	info, err := ResolvePluginInfo(ctx, db, reference)
	if err != nil {
		return protocol.PluginStorageConversationEntry{}, err
	}
	entry, err := readPluginConversationValue(ctx, db, reference.Name, session, key)
	if err == nil && entry.Present {
		entry.Value, err = plugins.ProjectStorageValue(plugins.StorageCollection(info, protocol.PluginStorageScopeConversation, key), entry.Value)
	}
	return entry, err
}

func pluginConversationAnchor(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn *protocol.TurnID) (*protocol.TurnID, error) {
	if turn != nil {
		var owner string
		err := db.QueryRowContext(ctx, "SELECT t.session FROM turns t JOIN conversation_turns c ON c.turn=t.id WHERE c.session=?1 AND t.id=?2",
			session.String(), turn.String()).Scan(&owner)
		if errors.Is(err, sql.ErrNoRows) || err == nil && owner != session.String() {
			return nil, fault(protocol.ErrorCodePermissionDenied, "plugin conversation turn is not owned by this session")
		}
		return turn, err
	}
	var latest string
	err := db.QueryRowContext(ctx, "SELECT c.turn FROM conversation_turns c JOIN turns t ON t.id=c.turn WHERE c.session=?1 ORDER BY t.rowid DESC LIMIT 1", session.String()).Scan(&latest)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var result protocol.TurnID
	if err := result.UnmarshalText([]byte(latest)); err != nil {
		return nil, err
	}
	return &result, nil
}

func WritePluginConversationValue(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, session protocol.SessionID, turn *protocol.TurnID, key string, value json.RawMessage, expectedRevision uint64) (protocol.PluginStorageConversationEntry, error) {
	info, err := ResolvePluginInfo(ctx, db, reference)
	if err != nil {
		return protocol.PluginStorageConversationEntry{}, err
	}
	current, err := readPluginConversationValue(ctx, db, reference.Name, session, key)
	if err != nil {
		return current, err
	}
	if err := pluginValueRevision(current.Revision, expectedRevision); err != nil {
		return current, err
	}
	if value == nil && current.Revision == 0 {
		return current, nil
	}
	if value != nil {
		var previous json.RawMessage
		if current.Present {
			previous = current.Value
		}
		value, err = plugins.NormalizeStorageValue(plugins.StorageCollection(info, protocol.PluginStorageScopeConversation, key), previous, value)
		if err != nil {
			return current, err
		}
	}
	anchor, err := pluginConversationAnchor(ctx, db, session, turn)
	if err != nil {
		return current, err
	}
	var count, total int64
	if err := db.QueryRowContext(ctx, `SELECT count(*),coalesce(sum(length(r.value)),0) FROM plugin_conversation_values v
JOIN plugin_conversation_records r ON r.sequence=v.record WHERE v.name=?1 AND v.session=?2 AND r.value IS NOT NULL`,
		reference.Name, session.String()).Scan(&count, &total); err != nil {
		return current, err
	}
	if current.Present {
		count--
		total -= int64(len(current.Value))
	}
	if value != nil {
		count++
		total += int64(len(value))
	}
	if count > 4096 || total > 128*1024*1024 {
		return current, fault(protocol.ErrorCodeBusy, "plugin storage is full")
	}
	revision, err := nextRevision(current.Revision)
	if err != nil {
		return current, err
	}
	record, err := NextCounter(ctx, db, CounterPluginRecords)
	if err != nil {
		return current, err
	}
	var body any
	if value != nil {
		body = []byte(value)
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO plugin_conversation_records(sequence,name,key,value) VALUES(?1,?2,?3,?4)", record, reference.Name, key, body); err != nil {
		return current, err
	}
	var anchorValue any
	if anchor != nil {
		anchorValue = anchor.String()
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO plugin_conversation_history(session,record,turn) VALUES(?1,?2,?3)", session.String(), record, anchorValue); err != nil {
		return current, err
	}
	if err := projectPluginConversation(ctx, db, reference.Name, session, key, revision, record, false); err != nil {
		return current, err
	}
	result := protocol.PluginStorageConversationEntry{Key: key, Revision: revision, Present: value != nil, Value: json.RawMessage("null")}
	if value != nil {
		result.Value = value
	}
	return result, nil
}

func projectPluginConversation(ctx context.Context, db *sql.Conn, name string, session protocol.SessionID, key string, revision uint64, record any, restored bool) error {
	_, err := db.ExecContext(ctx, `INSERT INTO plugin_conversation_values(name,session,key,revision,record,restored) VALUES(?1,?2,?3,?4,?5,?6)
ON CONFLICT(name,session,key) DO UPDATE SET revision=excluded.revision,record=excluded.record,restored=excluded.restored`,
		name, session.String(), key, revision, record, restored)
	return err
}

func RestorePluginConversation(ctx context.Context, db *sql.Conn, session protocol.SessionID) error {
	rows, err := db.QueryContext(ctx, `SELECT name,key,revision,NULL FROM plugin_conversation_values WHERE session=?1
UNION ALL SELECT r.name,r.key,0,max(h.record) FROM plugin_conversation_history h
JOIN plugin_conversation_records r ON r.sequence=h.record WHERE h.session=?1 GROUP BY r.name,r.key`, session.String())
	if err != nil {
		return err
	}
	type selected struct {
		name     string
		key      string
		revision uint64
		record   sql.NullInt64
	}
	values := map[[2]string]selected{}
	for rows.Next() {
		var value selected
		if err := rows.Scan(&value.name, &value.key, &value.revision, &value.record); err != nil {
			rows.Close()
			return err
		}
		identity := [2]string{value.name, value.key}
		previous := values[identity]
		value.revision = max(value.revision, previous.revision)
		if !value.record.Valid {
			value.record = previous.record
		}
		values[identity] = value
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return err
	}
	for _, value := range values {
		revision, err := nextRevision(value.revision)
		if err != nil {
			return err
		}
		if err := projectPluginConversation(ctx, db, value.name, session, value.key, revision, value.record, true); err != nil {
			return err
		}
	}
	return nil
}

func ForkPluginConversation(ctx context.Context, db *sql.Conn, source, target protocol.SessionID, through protocol.TurnID, preserveCurrent bool) error {
	_, err := db.ExecContext(ctx, `INSERT INTO plugin_conversation_history(session,record,turn)
SELECT ?2,h.record,h.turn FROM plugin_conversation_history h LEFT JOIN turns t ON t.id=h.turn WHERE h.session=?1 AND
(h.turn IS NULL OR t.rowid<=(SELECT rowid FROM turns WHERE id=?3) AND EXISTS(SELECT 1 FROM conversation_turns c WHERE c.session=?1 AND c.turn=h.turn))`,
		source.String(), target.String(), through.String())
	if err != nil {
		return err
	}
	if preserveCurrent {
		rows, err := db.QueryContext(ctx, "SELECT name,key,record FROM plugin_conversation_values WHERE session=?1", source.String())
		if err != nil {
			return err
		}
		type pointer struct {
			name, key string
			record    sql.NullInt64
		}
		pointers := []pointer{}
		for rows.Next() {
			var value pointer
			if err := rows.Scan(&value.name, &value.key, &value.record); err != nil {
				rows.Close()
				return err
			}
			pointers = append(pointers, value)
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return err
		}
		for _, value := range pointers {
			record := uint64(value.record.Int64)
			if !value.record.Valid {
				record, err = NextCounter(ctx, db, CounterPluginRecords)
				if err != nil {
					return err
				}
				if _, err := db.ExecContext(ctx, "INSERT INTO plugin_conversation_records(sequence,name,key,value) VALUES(?1,?2,?3,NULL)", record, value.name, value.key); err != nil {
					return err
				}
			}
			if _, err := db.ExecContext(ctx, "INSERT OR IGNORE INTO plugin_conversation_history(session,record,turn) VALUES(?1,?2,?3)", target.String(), record, through.String()); err != nil {
				return err
			}
		}
	}
	return RestorePluginConversation(ctx, db, target)
}

func RewindPluginConversation(ctx context.Context, db *sql.Conn, session protocol.SessionID, through *protocol.TurnID) error {
	var err error
	if through == nil {
		_, err = db.ExecContext(ctx, "DELETE FROM plugin_conversation_history WHERE session=?1", session.String())
	} else {
		_, err = db.ExecContext(ctx, "DELETE FROM plugin_conversation_history WHERE session=?1 AND turn IN (SELECT id FROM turns WHERE rowid>(SELECT rowid FROM turns WHERE id=?2))", session.String(), through.String())
	}
	if err != nil {
		return err
	}
	return RestorePluginConversation(ctx, db, session)
}
