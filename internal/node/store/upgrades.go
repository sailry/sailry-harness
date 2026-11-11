package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"reflect"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

var defaultHooksColumn = func() []byte {
	encoded, err := json.Marshal(protocol.DefaultHooksSettings())
	if err != nil {
		panic("default hook settings cannot be encoded")
	}
	return encoded
}()

func upgradeColumnDefault(table, name string) ([]byte, bool) {
	switch {
	case table == "turns" && name == "instructions":
		return []byte("[]"), true
	case table == "agent_approvals" && (name == "binding" || name == "prepared"):
		return []byte("null"), true
	case table == "agent_runs" && name == "hooks":
		return defaultHooksColumn, true
	default:
		return nil, false
	}
}

func matchesUpgradeDefault(table, name string, value []byte) bool {
	switch {
	case table == "turns" && name == "instructions":
		return bytes.Equal(value, []byte("[]"))
	case table == "agent_approvals" && (name == "binding" || name == "prepared"):
		var decoded any
		return json.Unmarshal(value, &decoded) == nil && decoded == nil
	case table == "agent_runs" && name == "hooks":
		var decoded protocol.HooksSettings
		return json.Unmarshal(value, &decoded) == nil && reflect.DeepEqual(decoded, protocol.DefaultHooksSettings())
	default:
		return false
	}
}

// Checkpoints recorded before they drew from the watermark took SQLite's own
// rowids. Nothing deletes a checkpoint, so none of those rowids was reused.
const checkpointWatermarkPending = "SELECT coalesce((SELECT max(rowid) FROM file_checkpoints),0)>coalesce((SELECT value FROM counters WHERE namespace='file_checkpoints'),0)"

const missingArtifactKind = "CASE WHEN json_valid(body) THEN json_type(body)='object' AND json_type(body,'$.kind') IS NULL ELSE 0 END"

// rawSecretRequest selects the admissions of secret-bearing commands that an
// earlier build stored as their serialized request.
var rawSecretRequest = func() string {
	var kinds []string
	for _, kind := range protocol.CommandKinds() {
		if admission.DigestOnly(kind) {
			kinds = append(kinds, "'"+kind+"'")
		}
	}
	return "CASE WHEN json_valid(CAST(body AS TEXT)) THEN json_extract(CAST(body AS TEXT),'$.command.kind') IN (" + strings.Join(kinds, ",") + ") ELSE 0 END"
}()

func ledgerSecretsPending(ctx context.Context, connection *sql.Conn) (bool, error) {
	var pending bool
	err := connection.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM requests WHERE "+rawSecretRequest+")").Scan(&pending)
	return pending, err
}

// digestLedgerSecrets replaces each such body by the digest that admission
// stores now, so replay, conflict detection and inspection are unchanged. The
// replaced bytes are overwritten rather than left in freed space.
func digestLedgerSecrets(ctx context.Context, connection *sql.Conn) error {
	rows, err := connection.QueryContext(ctx, "SELECT caller,id,body FROM requests WHERE "+rawSecretRequest)
	if err != nil {
		return err
	}
	type identity struct {
		caller []byte
		id     string
		body   []byte
	}
	var pending []identity
	for rows.Next() {
		var row identity
		if err := rows.Scan(&row.caller, &row.id, &row.body); err != nil {
			_ = rows.Close()
			return err
		}
		row.body = admission.CredentialBody(row.body)
		pending = append(pending, row)
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil || len(pending) == 0 {
		return err
	}
	var previous int
	if err := connection.QueryRowContext(ctx, "PRAGMA secure_delete").Scan(&previous); err != nil {
		return err
	}
	var enabled int
	if err := connection.QueryRowContext(ctx, "PRAGMA secure_delete=ON").Scan(&enabled); err != nil {
		return err
	}
	if enabled != 1 {
		return errors.New("secure deletion is unavailable; existing data was not changed")
	}
	for _, row := range pending {
		if _, err = connection.ExecContext(ctx, "UPDATE requests SET body=?3 WHERE caller=?1 AND id=?2", row.caller, row.id, row.body); err != nil {
			break
		}
	}
	mode := [...]string{"OFF", "ON", "FAST"}[min(max(previous, 0), 2)]
	var restored int
	return errors.Join(err, connection.QueryRowContext(ctx, "PRAGMA secure_delete="+mode).Scan(&restored))
}

// digestLedgerCopy applies the same replacement to a closed database copy.
func digestLedgerCopy(ctx context.Context, path string) error {
	handle, err := OpenHandle(ctx, path)
	if err != nil {
		return err
	}
	if err := handle.Write(ctx, func(connection *sql.Conn) error { return digestLedgerSecrets(ctx, connection) }); err != nil {
		return errors.Join(err, handle.Close())
	}
	// Pausing folds the log into the copy, whose pages still precede the
	// replacement, and truncates it.
	closed, err := handle.Pause(ctx)
	if err != nil {
		return errors.Join(err, handle.Close())
	}
	return closed.Close()
}

func upgradesNeeded(ctx context.Context, connection *sql.Conn) (bool, error) {
	for _, table := range []string{"artifacts", "artifact_revisions"} {
		var pending bool
		if err := connection.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM "+table+" WHERE "+missingArtifactKind+")").Scan(&pending); err != nil {
			return false, err
		}
		if pending {
			return true, nil
		}
	}
	var pending bool
	if err := connection.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM node WHERE singleton=1 AND (tool_metadata_names=0 OR tool_defaults=0))").Scan(&pending); err != nil || pending {
		return pending, err
	}
	if err := connection.QueryRowContext(ctx, checkpointWatermarkPending).Scan(&pending); err != nil || pending {
		return pending, err
	}
	return defaultsPending(ctx, connection)
}

func applyUpgrades(ctx context.Context, connection *sql.Conn) error {
	if err := upgradeToolMetadata(ctx, connection); err != nil {
		return err
	}
	if err := digestLedgerSecrets(ctx, connection); err != nil {
		return err
	}
	var pending bool
	if err := connection.QueryRowContext(ctx, checkpointWatermarkPending).Scan(&pending); err != nil {
		return err
	}
	if pending {
		if _, err := connection.ExecContext(ctx, "INSERT INTO counters(namespace,value) SELECT 'file_checkpoints',max(rowid) FROM file_checkpoints WHERE true ON CONFLICT(namespace) DO UPDATE SET value=excluded.value"); err != nil {
			return err
		}
	}
	for _, table := range []string{"artifacts", "artifact_revisions"} {
		if _, err := connection.ExecContext(ctx, "UPDATE "+table+" SET body=CAST(json_set(body,'$.kind',?1) AS BLOB) WHERE "+missingArtifactKind, protocol.ArtifactKindDocument); err != nil {
			return err
		}
	}
	if err := connection.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM node WHERE singleton=1 AND tool_defaults=0)").Scan(&pending); err != nil {
		return err
	}
	if pending {
		if err := upgradeRememberedTools(ctx, connection); err != nil {
			return err
		}
		_, err := connection.ExecContext(ctx, "UPDATE node SET tool_defaults=1 WHERE singleton=1")
		return err
	}
	return nil
}

func upgradeToolMetadata(ctx context.Context, connection *sql.Conn) error {
	var pending bool
	if err := connection.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM node WHERE singleton=1 AND tool_metadata_names=0)").Scan(&pending); err != nil || !pending {
		return err
	}
	rows, err := connection.QueryContext(ctx, "SELECT sequence,body FROM agent_events ORDER BY sequence")
	if err != nil {
		return err
	}
	type changedRecord struct {
		sequence int64
		body     []byte
	}
	var changed []changedRecord
	for rows.Next() {
		var sequence int64
		var encoded []byte
		if err := rows.Scan(&sequence, &encoded); err != nil {
			_ = rows.Close()
			return err
		}
		decoder := json.NewDecoder(bytes.NewReader(encoded))
		decoder.UseNumber()
		var body any
		if err := decoder.Decode(&body); err != nil {
			_ = rows.Close()
			return fmt.Errorf("invalid conversation record %d; existing data was not changed", sequence)
		}
		if err := decoder.Decode(new(any)); err != io.EOF {
			_ = rows.Close()
			return fmt.Errorf("invalid conversation record %d; existing data was not changed", sequence)
		}
		updated, err := upgradeRecordMetadata(body)
		if err != nil {
			_ = rows.Close()
			return err
		}
		if updated {
			encoded, err := json.Marshal(body)
			if err != nil {
				_ = rows.Close()
				return errors.New("conversation metadata could not be encoded")
			}
			changed = append(changed, changedRecord{sequence, encoded})
		}
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return err
	}
	for _, record := range changed {
		if _, err := connection.ExecContext(ctx, "UPDATE agent_events SET body=?1 WHERE sequence=?2", record.body, record.sequence); err != nil {
			return err
		}
	}
	_, err = connection.ExecContext(ctx, "UPDATE node SET tool_metadata_names=1 WHERE singleton=1")
	return err
}

func upgradeRecordMetadata(value any) (bool, error) {
	body, ok := value.(map[string]any)
	if !ok {
		return false, nil
	}
	switch body["kind"] {
	case "message":
		return upgradeMessageMetadata(body)
	case "compaction":
		summary, _ := body["summary"].(map[string]any)
		return upgradeMessageMetadata(summary)
	case "operation":
		if body["operation"] == "tool_meta" {
			return upgradeMetadataFields(body["patch"])
		}
	}
	return false, nil
}

func upgradeMessageMetadata(message map[string]any) (bool, error) {
	content, _ := message["content"].([]any)
	changed := false
	for _, value := range content {
		block, ok := value.(map[string]any)
		if !ok || block["type"] != "tool_request" {
			continue
		}
		updated, err := upgradeMetadataFields(block["tool_meta"])
		if err != nil {
			return false, err
		}
		changed = changed || updated
	}
	return changed, nil
}

func upgradeMetadataFields(value any) (bool, error) {
	metadata, ok := value.(map[string]any)
	if !ok {
		return false, nil
	}
	changed := false
	for _, names := range [][2]string{{"goose.executable", "sailry.executable"}, {"goose.external_dispatch", "sailry.external_dispatch"}} {
		previous, exists := metadata[names[0]]
		if !exists {
			continue
		}
		if current, exists := metadata[names[1]]; exists && !reflect.DeepEqual(current, previous) {
			return false, errors.New("conflicting tool execution metadata; existing data was not changed")
		}
		delete(metadata, names[0])
		metadata[names[1]] = previous
		changed = true
	}
	return changed, nil
}

func upgradeRememberedTools(ctx context.Context, connection *sql.Conn) error {
	const loadedTools = "sailry.context.loaded_tools"
	loaded := make(map[string][]string)
	rows, err := connection.QueryContext(ctx, "SELECT owner,body FROM agent_state WHERE key=?1", loadedTools)
	if err != nil {
		return err
	}
	for rows.Next() {
		var owner string
		var body []byte
		if err := rows.Scan(&owner, &body); err != nil {
			_ = rows.Close()
			return err
		}
		var entries []json.RawMessage
		if err := json.Unmarshal(body, &entries); err != nil || bytes.Equal(bytes.TrimSpace(body), []byte("null")) {
			_ = rows.Close()
			return errors.New("stored loaded tools must be a string array")
		}
		names := make([]string, 0, len(entries))
		for _, entry := range entries {
			var name *string
			if err := json.Unmarshal(entry, &name); err != nil || name == nil {
				_ = rows.Close()
				return errors.New("stored loaded tools must be a string array")
			}
			names = append(names, *name)
		}
		loaded[owner] = names
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return err
	}
	rows, err = connection.QueryContext(ctx, `SELECT DISTINCT h.session,json_extract(part.value,'$.name')
FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
JOIN json_each(e.body,'$.content') part
WHERE json_extract(part.value,'$.type')='tool_request'`)
	if err != nil {
		return err
	}
	missing := make(map[string]map[string]bool)
	for rows.Next() {
		var session, name string
		if err := rows.Scan(&session, &name); err != nil {
			_ = rows.Close()
			return err
		}
		if slices.Contains(loaded[session], name) {
			continue
		}
		if _, err := protocol.ParseSessionID(session); err != nil {
			_ = rows.Close()
			return err
		}
		if missing[session] == nil {
			missing[session] = make(map[string]bool)
		}
		missing[session][name] = true
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return err
	}
	for _, session := range sortedKeys(missing) {
		names := append(loaded[session], sortedKeys(missing[session])...)
		encoded, err := json.Marshal(names)
		if err != nil {
			return err
		}
		if _, err := connection.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", session, loadedTools, encoded); err != nil {
			return err
		}
	}
	return nil
}

type databaseCredentialTunnel struct {
	Host           string                     `json:"host"`
	Port           uint16                     `json:"port"`
	Username       string                     `json:"username"`
	Authentication protocol.SshAuthentication `json:"authentication"`
}

func (value *databaseCredentialTunnel) UnmarshalJSON(data []byte) error {
	type plain databaseCredentialTunnel
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, []string{"host", "port", "username", "authentication"}, true); err != nil {
		return err
	}
	*value = databaseCredentialTunnel(decoded)
	return nil
}

type databaseCredentialBinding struct {
	Connection protocol.DatabaseConnection `json:"connection"`
	Tunnel     *databaseCredentialTunnel   `json:"tunnel"`
}

func (value *databaseCredentialBinding) UnmarshalJSON(data []byte) error {
	type plain databaseCredentialBinding
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, []string{"connection"}, true); err != nil {
		return err
	}
	*value = databaseCredentialBinding(decoded)
	return nil
}

func reconcileDatabaseTargets(ctx context.Context, connection *sql.Conn) error {
	rows, err := connection.QueryContext(ctx, "SELECT body FROM database_profiles ORDER BY id")
	if err != nil {
		return err
	}
	var profiles []protocol.DatabaseProfile
	for rows.Next() {
		var body []byte
		if err := rows.Scan(&body); err != nil {
			_ = rows.Close()
			return err
		}
		var profile protocol.DatabaseProfile
		if err := json.Unmarshal(body, &profile); err != nil {
			_ = rows.Close()
			return err
		}
		profiles = append(profiles, profile)
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return err
	}
	for _, profile := range profiles {
		if profile.Connection.Kind == "ssh" {
			continue
		}
		binding, err := json.Marshal(databaseCredentialBinding{Connection: profile.Connection})
		if err != nil {
			return err
		}
		if _, err := connection.ExecContext(ctx, "UPDATE database_profiles SET password_target=?2 WHERE id=?1 AND password_target IS NULL", profile.ID.String(), binding); err != nil {
			return err
		}
	}
	return nil
}
