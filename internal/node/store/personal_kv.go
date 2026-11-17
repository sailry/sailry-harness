package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const personalKVOutboxLimit = 128 * 1024 * 1024

// Every legal group and receipt fits this derived encoding bound: 64 writes,
// base64 values, bounded indexes and escaped keys, plus the fixed header. It
// imposes no smaller transaction quota than the existing storage contract.
const MaxPersonalKVTransferBytes = 64*(4*((plugins.MaxValueBytes+2)/3)+maxPluginIndexBytes+2048) + 4096

type personalKVTransactionKey struct{}
type personalKVTransaction struct {
	user   protocol.User
	group  protocol.PersonalKVGroup
	writes map[string]protocol.PersonalKVWrite
}

func personalPluginSpace(ctx context.Context, db *sql.Conn, user protocol.UserID) (string, error) {
	current, cached, err := configurationCacheUser(ctx, db)
	if err != nil {
		return "", err
	}
	if cached && current.ID == user {
		return "controller:" + user.String(), nil
	}
	return "user:" + user.String(), nil
}

func beginPersonalKVTransaction(ctx context.Context, db *sql.Conn, node protocol.NodeID, request protocol.Request) (context.Context, *personalKVTransaction, error) {
	if _, ok := ctx.Value(personalKVTransactionKey{}).(*personalKVTransaction); ok {
		return ctx, nil, nil
	}
	user, cached, err := configurationCacheUser(ctx, db)
	if err != nil || !cached || request.Plugin == nil {
		return ctx, nil, err
	}
	tx := &personalKVTransaction{
		user: user, writes: map[string]protocol.PersonalKVWrite{},
		group: protocol.PersonalKVGroup{Controller: *user.Controller, Executor: node, Request: request.ID, Package: request.Plugin.Package},
	}
	return context.WithValue(ctx, personalKVTransactionKey{}, tx), tx, nil
}

// recordPersonalKVWrite runs inside the original command's transaction. Its
// first write preserves the source CAS; later writes only replace final content.
func recordPersonalKVWrite(ctx context.Context, db *sql.Conn, plugin string, entry protocol.PluginStorageEntry, index *protocol.PluginStorageIndex) error {
	if pluginValueScope(ctx) != protocol.PluginStorageScopeNode {
		return nil
	}
	tx, _ := ctx.Value(personalKVTransactionKey{}).(*personalKVTransaction)
	if tx == nil {
		user, cached, err := configurationCacheUser(ctx, db)
		if err != nil {
			return err
		}
		if cached {
			return fault(protocol.ErrorCodePermissionDenied, "personal writes require an admitted transaction")
		}
		_, err = db.ExecContext(ctx, `INSERT INTO personal_kv_clock(user,revision,sequence) VALUES(?1,1,0)
ON CONFLICT(user) DO UPDATE SET revision=revision+1`, user.ID.String())
		return err
	}
	if plugin != tx.group.Package.Name {
		return invalid("personal transaction contains another package")
	}
	write, exists := tx.writes[entry.Key]
	if !exists {
		write.Key = entry.Key
		err := db.QueryRowContext(ctx, "SELECT revision FROM personal_kv_base WHERE user=?1 AND plugin=?2 AND key=?3", tx.user.ID.String(), plugin, entry.Key).Scan(&write.BaseRevision)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return err
		}
		groups, err := personalKVPendingGroups(ctx, db, tx.user.ID)
		if err != nil {
			return err
		}
		for _, group := range groups {
			if group.Package.Name == plugin && slices.ContainsFunc(group.Writes, func(value protocol.PersonalKVWrite) bool { return value.Key == entry.Key }) {
				id := group.Request
				write.Predecessor = &id
			}
		}
	}
	write.Present, write.Index = entry.Present, index
	write.Value, write.ValueBytes = nil, nil
	if entry.Present {
		write.ValueBytes = bytes.Clone(entry.Value)
	}
	tx.writes[entry.Key] = write
	return nil
}

func finishPersonalKVTransaction(ctx context.Context, db *sql.Conn, tx *personalKVTransaction) error {
	if tx == nil || len(tx.writes) == 0 {
		return nil
	}
	for _, write := range tx.writes {
		tx.group.Writes = append(tx.group.Writes, write)
	}
	slices.SortFunc(tx.group.Writes, func(a, b protocol.PersonalKVWrite) int { return strings.Compare(a.Key, b.Key) })
	if _, err := db.ExecContext(ctx, `INSERT INTO personal_kv_clock(user,revision,sequence) VALUES(?1,0,1)
ON CONFLICT(user) DO UPDATE SET sequence=sequence+1`, tx.user.ID.String()); err != nil {
		return err
	}
	if err := db.QueryRowContext(ctx, "SELECT sequence FROM personal_kv_clock WHERE user=?1", tx.user.ID.String()).Scan(&tx.group.Sequence); err != nil {
		return err
	}
	var err error
	tx.group.Digest, err = configurationDigest(tx.group)
	if err != nil {
		return err
	}
	body, err := encode(tx.group)
	if err != nil {
		return err
	}
	if err := personalKVOutboxCapacity(ctx, db, tx.user.ID, tx.group.Package.Name, int64(len(body))); err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO personal_kv_outbox(user,request,sequence,digest,body) VALUES(?1,?2,?3,?4,?5)", tx.user.ID.String(), tx.group.Request.String(), tx.group.Sequence, tx.group.Digest, body)
	return err
}

func personalKVOutboxCapacity(ctx context.Context, db *sql.Conn, user protocol.UserID, plugin string, additional int64) error {
	var pending int64
	if err := db.QueryRowContext(ctx, "SELECT coalesce(sum(length(body)),0) FROM personal_kv_outbox WHERE user=?1 AND json_extract(body,'$.package.name')=?2", user.String(), plugin).Scan(&pending); err != nil {
		return err
	}
	if additional > personalKVOutboxLimit-pending {
		return fault(protocol.ErrorCodeBusy, "personal data pending writes are full")
	}
	return nil
}

func personalKVPendingGroups(ctx context.Context, db *sql.Conn, user protocol.UserID) ([]protocol.PersonalKVGroup, error) {
	return readBodies[protocol.PersonalKVGroup](ctx, db, "SELECT body FROM personal_kv_outbox WHERE user=?1 ORDER BY sequence", user.String())
}

func ReadPersonalKVPending(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, continuation *protocol.PersonalKVRead) (protocol.PersonalKVPending, error) {
	var result protocol.PersonalKVPending
	user, cached, err := configurationCacheUser(ctx, db)
	if err != nil {
		return result, err
	}
	if !cached || user.Controller == nil || *user.Controller != caller || caller == node {
		return result, fault(protocol.ErrorCodePermissionDenied, "personal data does not belong to this controller")
	}
	var request any
	var offset uint64
	if continuation != nil {
		if continuation.Request == (protocol.RequestID{}) || continuation.Offset >= MaxPersonalKVTransferBytes {
			return result, invalid("personal data continuation is invalid")
		}
		request, offset = continuation.Request.String(), continuation.Offset
	}
	page := protocol.PersonalKVPage{Offset: offset}
	var id string
	err = db.QueryRowContext(ctx, `SELECT request,sequence,digest,length(body),substr(body,?3+1,?4)
FROM personal_kv_outbox WHERE user=?1 AND (?2 IS NULL OR request=?2) ORDER BY sequence LIMIT 1`,
		user.ID.String(), request, offset, protocol.ConfigurationPageBytes).Scan(&id, &page.Sequence, &page.Digest, &page.Bytes, &page.Data)
	if errors.Is(err, sql.ErrNoRows) {
		if continuation != nil {
			return result, fault(protocol.ErrorCodeNotFound, "personal data transaction is unavailable")
		}
		return result, nil
	}
	if err != nil {
		return result, err
	}
	page.Request, err = protocol.ParseRequestID(id)
	if err != nil {
		return result, err
	}
	if page.Bytes == 0 || page.Bytes > MaxPersonalKVTransferBytes || offset >= page.Bytes {
		return result, invalid("personal data page is outside its transaction")
	}
	if continuation == nil && page.Bytes <= protocol.ConfigurationPageBytes {
		result.Group = new(protocol.PersonalKVGroup)
		if err := json.Unmarshal(page.Data, result.Group); err != nil {
			return result, err
		}
	} else {
		result.Page = &page
	}
	if continuation == nil {
		err = db.QueryRowContext(ctx, "SELECT coalesce(max(sequence),0) FROM personal_kv_outbox WHERE user=?1", user.ID.String()).Scan(&result.Through)
	}
	return result, err
}

func personalKVRecord(ctx context.Context, db *sql.Conn, space, plugin, key string) (protocol.PersonalKVRecord, error) {
	record := protocol.PersonalKVRecord{Plugin: plugin, Key: key}
	var value, index []byte
	err := db.QueryRowContext(ctx, "SELECT revision,value,index_data FROM plugin_space_values WHERE space=?1 AND name=?2 AND key=?3", space, plugin, key).Scan(&record.Revision, &value, &index)
	if errors.Is(err, sql.ErrNoRows) {
		return record, nil
	}
	if err != nil {
		return record, err
	}
	record.Present = value != nil
	if record.Present {
		record.ValueBytes = value
	}
	if index != nil {
		record.Index = new(protocol.PluginStorageIndex)
		err = json.Unmarshal(index, record.Index)
	}
	return record, err
}

func validatePersonalKVRecord(record protocol.PersonalKVRecord) error {
	if !plugins.ValidateName(record.Plugin) || pluginKey(record.Key, false) != nil || record.Present && (!json.Valid(record.ValueJSON()) || len(record.ValueJSON()) > plugins.MaxValueBytes) {
		return invalid("invalid personal data record")
	}
	if len(record.Value) != 0 && record.ValueBytes != nil {
		return invalid("personal data record has conflicting value encodings")
	}
	if !record.Present && (len(record.ValueJSON()) != 0 || record.Index != nil) {
		return invalid("deleted personal data has content")
	}
	if record.Index != nil {
		body, err := json.Marshal(record.Index)
		if err != nil || len(body) > maxPluginIndexBytes {
			return invalid("personal data index exceeds its limit")
		}
		return plugins.ValidateStorageIndex(*record.Index)
	}
	return nil
}

func savePersonalKVRecord(ctx context.Context, db *sql.Conn, space string, record protocol.PersonalKVRecord, revision uint64) error {
	if err := validatePersonalKVRecord(record); err != nil {
		return err
	}
	var value, index any
	if record.Present {
		value = []byte(record.ValueJSON())
	}
	if record.Index != nil {
		body, err := json.Marshal(record.Index)
		if err != nil {
			return err
		}
		index = body
	}
	if revision == 0 {
		_, err := db.ExecContext(ctx, "DELETE FROM plugin_space_values WHERE space=?1 AND name=?2 AND key=?3", space, record.Plugin, record.Key)
		return err
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO plugin_space_values(space,name,key,revision,value,index_data) VALUES(?1,?2,?3,?4,?5,?6)
ON CONFLICT(space,name,key) DO UPDATE SET revision=excluded.revision,value=excluded.value,index_data=excluded.index_data`, space, record.Plugin, record.Key, revision, value, index); err != nil {
		return err
	}
	return replacePluginSearch(context.WithValue(ctx, pluginSpaceKey{}, space), db, record.Plugin, record.Key, record.Index)
}

func personalKVReceipt(ctx context.Context, db *sql.Conn, user protocol.UserID, executor protocol.NodeID, request protocol.RequestID) (*protocol.PersonalKVReceipt, error) {
	return readBody[protocol.PersonalKVReceipt](ctx, db, "SELECT body FROM personal_kv_receipts WHERE user=?1 AND executor=?2 AND request=?3", user.String(), executor[:], request.String())
}

// ApplyPersonalKVGroup applies only the personal write set. Dispatch jobs,
// notifications, project data and admitted turns are never replayed here.
func ApplyPersonalKVGroup(ctx context.Context, db *sql.Conn, node, executor protocol.NodeID, group protocol.PersonalKVGroup) (protocol.PersonalKVReceipt, []protocol.Event, error) {
	receipt := protocol.PersonalKVReceipt{Executor: executor, Request: group.Request, Sequence: group.Sequence, Digest: group.Digest, Records: []protocol.PersonalKVRecord{}}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return receipt, nil, err
	}
	if !user.Local || group.Controller != node || group.Executor != executor || group.Sequence == 0 || len(group.Writes) == 0 || len(group.Writes) > 64 || !plugins.ValidateName(group.Package.Name) {
		return receipt, nil, invalid("invalid personal data transaction")
	}
	copy := group
	copy.Digest = ""
	digest, err := configurationDigest(copy)
	if err != nil || group.Digest != digest {
		return receipt, nil, invalid("personal data transaction digest differs")
	}
	previous, err := personalKVReceipt(ctx, db, user.ID, executor, group.Request)
	if err != nil {
		return receipt, nil, err
	}
	if previous != nil {
		if previous.Digest != group.Digest {
			return receipt, nil, fault(protocol.ErrorCodeConflict, "personal data request identity was reused")
		}
		return *previous, nil, nil
	}
	var sequence uint64
	if err := db.QueryRowContext(ctx, "SELECT coalesce(max(sequence),0) FROM personal_kv_receipts WHERE user=?1 AND executor=?2", user.ID.String(), executor[:]).Scan(&sequence); err != nil {
		return receipt, nil, err
	}
	if group.Sequence != sequence+1 {
		return receipt, nil, fault(protocol.ErrorCodeRevisionConflict, "personal data transactions must be reconciled in order")
	}
	space := "user:" + user.ID.String()
	seen := map[string]bool{}
	receipt.Applied = true
	for _, write := range group.Writes {
		if seen[write.Key] {
			return receipt, nil, invalid("personal data transaction repeats a key")
		}
		seen[write.Key] = true
		record := protocol.PersonalKVRecord{Plugin: group.Package.Name, Key: write.Key, Present: write.Present, Value: write.Value, ValueBytes: write.ValueBytes, Index: write.Index}
		if err := validatePersonalKVRecord(record); err != nil {
			return receipt, nil, err
		}
		expected := write.BaseRevision
		if write.Predecessor != nil {
			predecessor, err := personalKVReceipt(ctx, db, user.ID, executor, *write.Predecessor)
			if err != nil {
				return receipt, nil, err
			}
			if predecessor == nil || !predecessor.Applied {
				receipt.Applied, receipt.Reason = false, "predecessor_conflict"
			} else {
				found := false
				for _, value := range predecessor.Records {
					if value.Plugin == record.Plugin && value.Key == record.Key {
						expected, found = value.Revision, true
					}
				}
				if !found {
					return receipt, nil, invalid("personal data predecessor does not write this key")
				}
			}
		}
		current, err := personalKVRecord(ctx, db, space, record.Plugin, record.Key)
		if err != nil {
			return receipt, nil, err
		}
		if current.Revision != expected {
			receipt.Applied, receipt.Reason = false, "source_conflict"
		}
		receipt.Records = append(receipt.Records, current)
	}
	if receipt.Applied {
		var count, total int64
		if err := db.QueryRowContext(ctx, "SELECT count(*),coalesce(sum(length(value)+coalesce(length(index_data),0)),0) FROM plugin_space_values WHERE space=?1 AND name=?2 AND value IS NOT NULL", space, group.Package.Name).Scan(&count, &total); err != nil {
			return receipt, nil, err
		}
		for index, write := range group.Writes {
			before := receipt.Records[index]
			if before.Present {
				count--
				size, _ := json.Marshal(before.Index)
				total -= int64(len(before.ValueJSON()))
				if before.Index != nil {
					total -= int64(len(size))
				}
			}
			if write.Present {
				count++
				size, _ := json.Marshal(write.Index)
				total += int64(len(write.ValueJSON()))
				if write.Index != nil {
					total += int64(len(size))
				}
			}
		}
		if count > maxPluginKeys || total > personalKVOutboxLimit {
			receipt.Applied, receipt.Reason = false, "source_capacity"
		}
	}
	if receipt.Applied {
		for index, write := range group.Writes {
			record := &receipt.Records[index]
			record.Revision, err = nextRevision(record.Revision)
			if err != nil {
				return receipt, nil, err
			}
			record.Value, record.ValueBytes, record.Index, record.Present = write.Value, write.ValueBytes, write.Index, write.Present
			if err := savePersonalKVRecord(ctx, db, space, *record, record.Revision); err != nil {
				return receipt, nil, err
			}
		}
		if _, err := db.ExecContext(ctx, `INSERT INTO personal_kv_clock(user,revision,sequence) VALUES(?1,1,0)
ON CONFLICT(user) DO UPDATE SET revision=revision+1`, user.ID.String()); err != nil {
			return receipt, nil, err
		}
	}
	body, err := encode(receipt)
	if err != nil {
		return receipt, nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO personal_kv_receipts(user,executor,request,sequence,digest,body) VALUES(?1,?2,?3,?4,?5,?6)", user.ID.String(), executor[:], group.Request.String(), group.Sequence, group.Digest, body); err != nil {
		return receipt, nil, err
	}
	events := []protocol.Event{}
	if receipt.Applied {
		events = append(events, protocol.Event{Kind: "plugin_values_changed", Data: protocol.EventPluginValuesChangedPayload{Name: group.Package.Name}})
		changed, err := PersonalKnowledgeChanged(ctx, db, group.Package.Name)
		if err != nil {
			return receipt, nil, err
		}
		if changed {
			events = append(events, protocol.Event{Kind: "knowledge_collections_changed"})
		}
	} else {
		notice, err := insertNotice(ctx, db, group.Package.Name, protocol.NotificationDraft{
			Title: group.Package.Name, Message: group.Package.Name, Kind: protocol.NotificationKindWarning,
			Details: &protocol.NotificationDetails{Kind: "personal_kv_conflict", Data: &protocol.PersonalKVConflict{Package: group.Package.Name, Request: group.Request, Reason: receipt.Reason}},
		}, time.Now().UnixMilli())
		if err != nil {
			return receipt, nil, err
		}
		events = append(events, protocol.Event{Kind: "notification_changed", Data: notice})
	}
	return receipt, events, nil
}

func savePersonalKVBase(ctx context.Context, db *sql.Conn, user protocol.UserID, record protocol.PersonalKVRecord) error {
	if err := validatePersonalKVRecord(record); err != nil {
		return err
	}
	var value, index any
	if record.Present {
		value = []byte(record.ValueJSON())
	}
	if record.Index != nil {
		raw, err := encode(record.Index)
		if err != nil {
			return err
		}
		index = raw
	}
	_, err := db.ExecContext(ctx, `INSERT INTO personal_kv_base(user,plugin,key,revision,value,index_data) VALUES(?1,?2,?3,?4,?5,?6)
ON CONFLICT(user,plugin,key) DO UPDATE SET revision=excluded.revision,value=excluded.value,index_data=excluded.index_data WHERE excluded.revision>=revision`, user.String(), record.Plugin, record.Key, record.Revision, value, index)
	return err
}

func materializePersonalKV(ctx context.Context, db *sql.Conn, user protocol.UserID, plugin, key string, groups []protocol.PersonalKVGroup) (bool, error) {
	var value, index []byte
	record := protocol.PersonalKVRecord{Plugin: plugin, Key: key}
	err := db.QueryRowContext(ctx, "SELECT revision,value,index_data FROM personal_kv_base WHERE user=?1 AND plugin=?2 AND key=?3", user.String(), plugin, key).Scan(&record.Revision, &value, &index)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return false, err
	}
	record.Present = value != nil
	if record.Present {
		record.ValueBytes = value
	}
	if index != nil {
		record.Index = new(protocol.PluginStorageIndex)
		if err := json.Unmarshal(index, record.Index); err != nil {
			return false, err
		}
	}
	for _, group := range groups {
		if group.Package.Name != plugin {
			continue
		}
		for _, write := range group.Writes {
			if write.Key == key {
				record.Value, record.ValueBytes, record.Index, record.Present = write.Value, write.ValueBytes, write.Index, write.Present
			}
		}
	}
	space := "controller:" + user.String()
	current, err := personalKVRecord(ctx, db, space, plugin, key)
	if err != nil {
		return false, err
	}
	before, _ := json.Marshal(current.Index)
	after, _ := json.Marshal(record.Index)
	if (current.Revision > 0 || record.Revision == 0) && current.Present == record.Present && bytes.Equal(current.ValueJSON(), record.ValueJSON()) && bytes.Equal(before, after) {
		return false, nil
	}
	revision, err := nextRevision(current.Revision)
	if err != nil {
		return false, err
	}
	return true, savePersonalKVRecord(ctx, db, space, record, revision)
}

func AcknowledgePersonalKV(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, receipt protocol.PersonalKVReceipt) ([]protocol.Event, error) {
	user, cached, err := configurationCacheUser(ctx, db)
	if err != nil {
		return nil, err
	}
	if !cached || user.Controller == nil || *user.Controller != caller || receipt.Executor != node {
		return nil, fault(protocol.ErrorCodePermissionDenied, "personal data receipt has another controller")
	}
	group, err := readBody[protocol.PersonalKVGroup](ctx, db, "SELECT body FROM personal_kv_outbox WHERE user=?1 AND request=?2", user.ID.String(), receipt.Request.String())
	if err != nil {
		return nil, err
	}
	if group == nil {
		previous, err := personalKVReceipt(ctx, db, user.ID, node, receipt.Request)
		if err != nil {
			return nil, err
		}
		if previous != nil && previous.Digest == receipt.Digest {
			return nil, nil
		}
		return nil, fault(protocol.ErrorCodeNotFound, "personal data transaction is unavailable")
	}
	if group.Digest != receipt.Digest || group.Sequence != receipt.Sequence || len(group.Writes) != len(receipt.Records) {
		return nil, invalid("personal data receipt differs from its transaction")
	}
	for index, record := range receipt.Records {
		if record.Plugin != group.Package.Name || record.Key != group.Writes[index].Key {
			return nil, invalid("personal data receipt has another write set")
		}
		if err := savePersonalKVBase(ctx, db, user.ID, record); err != nil {
			return nil, err
		}
	}
	body, err := encode(receipt)
	if err != nil {
		return nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO personal_kv_receipts(user,executor,request,sequence,digest,body) VALUES(?1,?2,?3,?4,?5,?6)", user.ID.String(), node[:], receipt.Request.String(), receipt.Sequence, receipt.Digest, body); err != nil {
		return nil, err
	}
	if _, err := db.ExecContext(ctx, "DELETE FROM personal_kv_outbox WHERE user=?1 AND request=?2", user.ID.String(), receipt.Request.String()); err != nil {
		return nil, err
	}
	groups, err := personalKVPendingGroups(ctx, db, user.ID)
	if err != nil {
		return nil, err
	}
	changed := false
	for _, record := range receipt.Records {
		value, err := materializePersonalKV(ctx, db, user.ID, record.Plugin, record.Key, groups)
		if err != nil {
			return nil, err
		}
		changed = changed || value
	}
	if !changed {
		return nil, nil
	}
	return []protocol.Event{{Kind: "plugin_values_changed", Data: protocol.EventPluginValuesChangedPayload{Name: group.Package.Name}}}, nil
}

func ReadPersonalKVSnapshot(ctx context.Context, db *sql.Conn, generation *uint64, after *protocol.PersonalKVCursor) (protocol.PersonalKVSnapshot, error) {
	page := protocol.PersonalKVSnapshot{After: after, Records: []protocol.PersonalKVRecord{}}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return page, err
	}
	if !user.Local {
		return page, fault(protocol.ErrorCodePermissionDenied, "personal data source is not local")
	}
	err = db.QueryRowContext(ctx, "SELECT revision FROM personal_kv_clock WHERE user=?1", user.ID.String()).Scan(&page.Generation)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return page, err
	}
	if generation != nil && *generation != page.Generation {
		return page, fault(protocol.ErrorCodeRevisionConflict, "personal data snapshot changed")
	}
	cursor := protocol.PersonalKVCursor{}
	if after != nil {
		cursor = *after
	}
	rows, err := db.QueryContext(ctx, `SELECT name,key,revision,value,index_data FROM plugin_space_values
WHERE space=?1 AND (name>?2 OR name=?2 AND key>?3) ORDER BY name,key LIMIT 65`, "user:"+user.ID.String(), cursor.Plugin, cursor.Key)
	if err != nil {
		return page, err
	}
	size := 0
	for rows.Next() {
		var record protocol.PersonalKVRecord
		var value, index []byte
		if err := rows.Scan(&record.Plugin, &record.Key, &record.Revision, &value, &index); err != nil {
			rows.Close()
			return page, err
		}
		if len(page.Records) == 64 || len(page.Records) > 0 && size+len(value)+len(index)+512 > 512*1024 {
			last := page.Records[len(page.Records)-1]
			page.Next = &protocol.PersonalKVCursor{Plugin: last.Plugin, Key: last.Key}
			break
		}
		record.Present = value != nil
		if record.Present {
			record.ValueBytes = value
		}
		if index != nil {
			record.Index = new(protocol.PluginStorageIndex)
			if err := json.Unmarshal(index, record.Index); err != nil {
				rows.Close()
				return page, err
			}
		}
		page.Records = append(page.Records, record)
		size += len(value) + len(index) + 512
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return page, err
	}
	var current uint64
	err = db.QueryRowContext(ctx, "SELECT revision FROM personal_kv_clock WHERE user=?1", user.ID.String()).Scan(&current)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return page, err
	}
	if current != page.Generation {
		return page, fault(protocol.ErrorCodeRevisionConflict, "personal data snapshot changed")
	}
	return page, nil
}

func StagePersonalKVSnapshot(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, page protocol.PersonalKVSnapshot) ([]protocol.Event, error) {
	user, cached, err := configurationCacheUser(ctx, db)
	if err != nil {
		return nil, err
	}
	if !cached || user.Controller == nil || *user.Controller != caller || caller == node || len(page.Records) > 64 {
		return nil, fault(protocol.ErrorCodePermissionDenied, "personal data snapshot has another controller")
	}
	if page.After == nil {
		if _, err := db.ExecContext(ctx, "DELETE FROM personal_kv_snapshot_records WHERE user=?1", user.ID.String()); err != nil {
			return nil, err
		}
		if _, err := db.ExecContext(ctx, `INSERT INTO personal_kv_snapshot(user,generation,cursor) VALUES(?1,?2,NULL)
ON CONFLICT(user) DO UPDATE SET generation=excluded.generation,cursor=NULL`, user.ID.String(), page.Generation); err != nil {
			return nil, err
		}
	} else {
		var generation uint64
		var cursor []byte
		if err := db.QueryRowContext(ctx, "SELECT generation,cursor FROM personal_kv_snapshot WHERE user=?1", user.ID.String()).Scan(&generation, &cursor); err != nil {
			return nil, err
		}
		expected, err := encode(page.After)
		if err != nil {
			return nil, err
		}
		if generation != page.Generation || !bytes.Equal(expected, cursor) {
			return nil, fault(protocol.ErrorCodeRevisionConflict, "personal data snapshot page is out of order")
		}
	}
	last := page.After
	for _, record := range page.Records {
		if err := validatePersonalKVRecord(record); err != nil {
			return nil, err
		}
		if last != nil && (record.Plugin < last.Plugin || record.Plugin == last.Plugin && record.Key <= last.Key) {
			return nil, invalid("personal data snapshot is not ordered")
		}
		last = &protocol.PersonalKVCursor{Plugin: record.Plugin, Key: record.Key}
		body, err := encode(record)
		if err != nil {
			return nil, err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO personal_kv_snapshot_records(user,plugin,key,body) VALUES(?1,?2,?3,?4)", user.ID.String(), record.Plugin, record.Key, body); err != nil {
			return nil, err
		}
	}
	if page.Next != nil {
		if last == nil || *last != *page.Next {
			return nil, invalid("personal data snapshot cursor differs from its last record")
		}
		cursor, err := encode(page.Next)
		if err != nil {
			return nil, err
		}
		_, err = db.ExecContext(ctx, "UPDATE personal_kv_snapshot SET cursor=?2 WHERE user=?1", user.ID.String(), cursor)
		return nil, err
	}
	keys := map[protocol.PersonalKVCursor]bool{}
	rows, err := db.QueryContext(ctx, "SELECT name,key FROM plugin_space_values WHERE space=?1", "controller:"+user.ID.String())
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var key protocol.PersonalKVCursor
		if err := rows.Scan(&key.Plugin, &key.Key); err != nil {
			rows.Close()
			return nil, err
		}
		keys[key] = true
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	records, err := readBodies[protocol.PersonalKVRecord](ctx, db, "SELECT body FROM personal_kv_snapshot_records WHERE user=?1 ORDER BY plugin,key", user.ID.String())
	if err != nil {
		return nil, err
	}
	groups, err := personalKVPendingGroups(ctx, db, user.ID)
	if err != nil {
		return nil, err
	}
	if _, err := db.ExecContext(ctx, "DELETE FROM personal_kv_base WHERE user=?1", user.ID.String()); err != nil {
		return nil, err
	}
	for _, record := range records {
		if err := savePersonalKVBase(ctx, db, user.ID, record); err != nil {
			return nil, err
		}
		keys[protocol.PersonalKVCursor{Plugin: record.Plugin, Key: record.Key}] = true
	}
	changed := map[string]bool{}
	for key := range keys {
		materialized, err := materializePersonalKV(ctx, db, user.ID, key.Plugin, key.Key, groups)
		if err != nil {
			return nil, err
		}
		changed[key.Plugin] = changed[key.Plugin] || materialized
	}
	if _, err := db.ExecContext(ctx, "DELETE FROM personal_kv_snapshot_records WHERE user=?1", user.ID.String()); err != nil {
		return nil, err
	}
	if _, err := db.ExecContext(ctx, "DELETE FROM personal_kv_snapshot WHERE user=?1", user.ID.String()); err != nil {
		return nil, err
	}
	events := []protocol.Event{}
	for name, value := range changed {
		if value {
			events = append(events, protocol.Event{Kind: "plugin_values_changed", Data: protocol.EventPluginValuesChangedPayload{Name: name}})
		}
	}
	slices.SortFunc(events, func(a, b protocol.Event) int {
		return strings.Compare(a.Data.(protocol.EventPluginValuesChangedPayload).Name, b.Data.(protocol.EventPluginValuesChangedPayload).Name)
	})
	return events, nil
}
