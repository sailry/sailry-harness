package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// Transport records keep the source identity and digest. Database identities
// belong to the receiving user, because these tables use global primary keys.
// Persisting the association also preserves captured IDs through user merges.
func configurationLocalID(ctx context.Context, db *sql.Conn, category, sourceID string) (string, error) {
	switch category {
	case "providers", "roles", "teams", "ssh_profiles", "database_profiles", "connections":
	default:
		return sourceID, nil
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return "", err
	}
	var local string
	var materialized bool
	err = db.QueryRowContext(ctx, "SELECT local_id,materialized FROM configuration_follow_records WHERE user=?1 AND category=?2 AND id=?3", user.ID.String(), category, sourceID).Scan(&local, &materialized)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return "", err
	}
	if local != "" {
		return local, nil
	}
	if materialized {
		return sourceID, nil // Existing replicas predate the explicit association.
	}
	table := category
	if category == "connections" {
		table = "plugin_connections"
	}
	var owned bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM "+table+" WHERE id=?1 AND user=?2)", sourceID, user.ID.String()).Scan(&owned); err != nil {
		return "", err
	}
	if owned {
		return sourceID, nil
	}
	id, err := protocol.ParseRequestID(sourceID)
	if err != nil {
		return "", invalid("configuration source identity is invalid")
	}
	var digest [32]byte
	blake3.DeriveKey(digest[:], "Sailry configuration namespace v1", []byte(user.ID.String()+"/"+category+"/"+sourceID))
	copy(id[:], digest[:16])
	id[6], id[8] = id[6]&0x0f|0x80, id[8]&0x3f|0x80
	return id.String(), nil
}

// localConfigurationRecord validates the original bytes before translating
// declared references. The caller stores and acknowledges the original record.
func localConfigurationRecord(ctx context.Context, db *sql.Conn, category string, record protocol.ConfigurationFollowRecord) (protocol.ConfigurationFollowRecord, error) {
	local := record
	var err error
	local.ID, err = configurationLocalID(ctx, db, category, record.ID)
	if err != nil {
		return local, err
	}
	mapID := func(category string, value [16]byte) ([16]byte, error) {
		id, err := configurationLocalID(ctx, db, category, protocol.RequestID(value).String())
		if err != nil {
			return value, err
		}
		mapped, err := protocol.ParseRequestID(id)
		return [16]byte(mapped), err
	}
	mapModel := func(model *protocol.RoleModel) error {
		if model == nil {
			return nil
		}
		id, err := mapID("providers", model.Provider)
		model.Provider = protocol.ProviderID(id)
		return err
	}
	var body any
	switch category {
	case "providers":
		var value protocol.ConversationProvider
		if err = decodeConfigurationRecord(record, &value); err != nil {
			return local, err
		}
		if value.ID.String() != record.ID {
			return local, invalid("provider identity does not match manifest")
		}
		value.ID, err = protocol.ParseProviderID(local.ID)
		body = value
	case "roles":
		var value protocol.RoleProfile
		if err = decodeConfigurationRecord(record, &value); err != nil {
			return local, err
		}
		if value.ID.String() != record.ID {
			return local, invalid("role identity does not match manifest")
		}
		local.ID, err = configurationKeyID(ctx, db, category, record.ID, local.ID, value.Key)
		if err != nil {
			return local, err
		}
		value.ID, err = protocol.ParseRoleID(local.ID)
		if err == nil {
			err = mapModel(value.Model)
		}
		body = value
	case "teams":
		var value protocol.TeamProfile
		if err = decodeConfigurationRecord(record, &value); err != nil {
			return local, err
		}
		if value.ID.String() != record.ID {
			return local, invalid("team identity does not match manifest")
		}
		local.ID, err = configurationKeyID(ctx, db, category, record.ID, local.ID, value.Key)
		if err != nil {
			return local, err
		}
		value.ID, err = protocol.ParseTeamID(local.ID)
		if err == nil {
			err = mapModel(value.Model)
		}
		for index, member := range value.Members {
			if err != nil {
				break
			}
			var id [16]byte
			id, err = mapID("roles", member)
			value.Members[index] = protocol.RoleID(id)
		}
		body = value
	case "ssh_profiles":
		var value protocol.SshProfile
		if err = decodeConfigurationRecord(record, &value); err != nil {
			return local, err
		}
		if value.ID.String() != record.ID {
			return local, invalid("SSH identity does not match manifest")
		}
		value.ID, err = protocol.ParseSshID(local.ID)
		value.Sharing = nil
		body = value
	case "database_profiles":
		var value protocol.DatabaseProfile
		if err = decodeConfigurationRecord(record, &value); err != nil {
			return local, err
		}
		if value.ID.String() != record.ID {
			return local, invalid("database identity does not match manifest")
		}
		value.ID, err = protocol.ParseDatabaseID(local.ID)
		value.Sharing = nil
		if value.Connection.Kind == "ssh" && err == nil {
			payload, ok := value.Connection.Data.(*protocol.DatabaseConnectionSshPayload)
			if !ok {
				return local, invalid("SSH database connection is invalid")
			}
			id, mapErr := mapID("ssh_profiles", payload.Ssh)
			payload.Ssh, err = protocol.SshID(id), mapErr
		}
		body = value
	case "defaults":
		var value protocol.Defaults
		if err = decodeConfigurationRecord(record, &value); err != nil {
			return local, err
		}
		if value.Config != nil {
			id, mapErr := mapID("providers", value.Config.Provider)
			value.Config.Provider, err = protocol.ProviderID(id), mapErr
		}
		body = value
	case "connections":
		var value pluginFollowConnection
		if err = decodeConfigurationRecord(record, &value); err != nil {
			return local, err
		}
		if value.ID.String() != record.ID {
			return local, invalid("connection identity does not match manifest")
		}
		value.ID, err = protocol.ParseRequestID(local.ID)
		body = value
	default:
		return local, nil
	}
	if err != nil {
		return local, err
	}
	local.Body, err = json.Marshal(body)
	if err == nil {
		local.Digest = ""
		local.Digest, err = configurationDigest(local)
	}
	return local, err
}

// An initial source role/team replaces the matching key in its own namespace,
// retaining that record's identity for already captured references.
func configurationKeyID(ctx context.Context, db *sql.Conn, category, source, local, key string) (string, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return "", err
	}
	var mapped bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM configuration_follow_records WHERE user=?1 AND category=?2 AND id=?3 AND (local_id!='' OR materialized=1))", user.ID.String(), category, source).Scan(&mapped); err != nil {
		return "", err
	}
	if mapped || user.Local {
		return local, nil
	}
	var existing string
	err = db.QueryRowContext(ctx, "SELECT id FROM "+category+" WHERE user=?1 AND key=?2", user.ID.String(), configurationKey(user, key)).Scan(&existing)
	if errors.Is(err, sql.ErrNoRows) {
		return local, nil
	}
	return existing, err
}
