package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func configurationDigest(value any) (string, error) {
	body, err := encode(value)
	if err != nil {
		return "", err
	}
	sum := blake3.Sum256(body)
	return hex.EncodeToString(sum[:]), nil
}

// ExportConfigurationManifest exports only the source Node's local personal
// rows. A receiver's replicas never become another user's onward source.
func ExportConfigurationManifest(ctx context.Context, db *sql.Conn, node, host protocol.NodeID, category string, secrets bool, declarations ...map[string]protocol.PluginMcpConfiguration) (protocol.ConfigurationManifest, error) {
	value := protocol.ConfigurationManifest{Source: node, Category: category, Records: []protocol.ConfigurationFollowRecord{}}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return value, err
	}
	if !user.Local {
		return value, fault(protocol.ErrorCodePermissionDenied, "only local configuration can be published")
	}
	add := func(id string, revision uint64, body any, secret *protocol.Secret, expiry *uint64) error {
		origin, err := configurationOrigin(ctx, db, category, id)
		if err != nil {
			return err
		}
		if origin != nil {
			return nil
		}
		raw, err := encode(body)
		if err != nil {
			return err
		}
		record := protocol.ConfigurationFollowRecord{ID: id, Revision: max(1, revision), Body: raw, Secret: secret, ExpiresAtMs: expiry}
		record.Digest, err = configurationDigest(record)
		if err != nil {
			return err
		}
		value.Records = append(value.Records, record)
		return nil
	}
	switch category {
	case "providers":
		values, err := providerConfigurations(ctx, db)
		if err != nil {
			return value, err
		}
		for _, saved := range values {
			if !saved.Listed || saved.Provider == nil || saved.Provider.Owner == nil || *saved.Provider.Owner != user.ID {
				continue
			}
			provider := *saved.Provider
			provider.Owner, provider.Shared, provider.Source = nil, false, nil
			provider.Consent = nil
			var key *protocol.Secret
			var expiry *uint64
			if secrets && provider.Credential != nil && transferable(provider.Authentication) == nil {
				auth, ok := saved.Authorizations[provider.Credential.ID]
				if ok {
					key, expiry = auth.Key, auth.ExpiresAtMs
				}
			}
			if key == nil {
				provider.Credential = nil
			}
			if err := add(provider.ID.String(), provider.Revision, provider, key, expiry); err != nil {
				return value, err
			}
		}
	case "roles":
		values, err := ReadRoles(ctx, db)
		if err != nil {
			return value, err
		}
		for _, item := range values {
			item.Source = nil
			if err := add(item.ID.String(), item.Revision, item, nil, nil); err != nil {
				return value, err
			}
		}
	case "teams":
		values, err := ReadTeams(ctx, db)
		if err != nil {
			return value, err
		}
		for _, item := range values {
			item.Source = nil
			if err := add(item.ID.String(), item.Revision, item, nil, nil); err != nil {
				return value, err
			}
		}
	case "ssh_profiles":
		values, err := ListSsh(ctx, db)
		if err != nil {
			return value, err
		}
		for _, item := range values {
			if item.Authentication == protocol.SshAuthenticationKeyPath {
				continue
			}
			item.Source = nil
			item.HostKey = nil
			var key *protocol.Secret
			if secrets {
				_, credential, err := ReadSsh(ctx, db, item.ID, item.Revision)
				if err != nil {
					return value, err
				}
				body, err := encode(credential)
				if err != nil {
					return value, err
				}
				secret := protocol.NewSecret(string(body))
				key = &secret
			}
			if err := add(item.ID.String(), item.Revision, item, key, nil); err != nil {
				return value, err
			}
		}
	case "database_profiles":
		values, err := ListDatabases(ctx, db)
		if err != nil {
			return value, err
		}
		for _, item := range values {
			item.Source = nil
			var key *protocol.Secret
			if secrets {
				var raw string
				if err := db.QueryRowContext(ctx, "SELECT password FROM database_profiles WHERE id=?1", item.ID.String()).Scan(&raw); err != nil {
					return value, err
				}
				secret := protocol.NewSecret(raw)
				key = &secret
			}
			if err := add(item.ID.String(), item.Revision, item, key, nil); err != nil {
				return value, err
			}
		}
	case "defaults":
		item, err := ReadDefaults(ctx, db)
		if err != nil {
			return value, err
		}
		if item.Config != nil {
			item.Source = nil
			copy := *item.Config
			copy.Resource, copy.Assistant = nil, nil
			if !secrets {
				copy.Credential = nil
			}
			item.Config = &copy
			if err := add("default", item.Revision, item, nil, nil); err != nil {
				return value, err
			}
		}
	case "team_settings":
		item, err := ReadTeamSettings(ctx, db)
		if err != nil {
			return value, err
		}
		item.Source = nil
		if err := add("default", item.Revision, item, nil, nil); err != nil {
			return value, err
		}
	case "plugins", "plugin_settings", "connections":
		declared := map[string]protocol.PluginMcpConfiguration{}
		if len(declarations) > 0 {
			declared = declarations[0]
		}
		records, err := exportPluginFollowRecords(ctx, db, category, secrets, declared)
		if err != nil {
			return value, err
		}
		value.Records = records
	default:
		return value, invalid("unsupported configuration category")
	}
	digest, err := configurationManifestDigest(value.Records)
	if err != nil {
		return value, err
	}
	var previous string
	err = db.QueryRowContext(ctx, "SELECT revision,digest FROM configuration_follow_versions WHERE user=?1 AND category=?2 AND source=?3", user.ID.String(), category, host[:]).Scan(&value.Revision, &previous)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	if previous != digest {
		value.Revision, err = nextRevision(value.Revision)
		if err != nil {
			return value, err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_versions(user,category,source,revision,digest) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(user,category,source) DO UPDATE SET revision=excluded.revision,digest=excluded.digest", user.ID.String(), category, host[:], value.Revision, digest)
	}
	return value, err
}

func configurationManifestDigest(records []protocol.ConfigurationFollowRecord) (string, error) {
	metadata := make([]protocol.ConfigurationFollowRecord, len(records))
	for index, record := range records {
		metadata[index] = protocol.ConfigurationFollowRecord{ID: record.ID, Revision: record.Revision, Digest: record.Digest}
	}
	slices.SortFunc(metadata, func(a, b protocol.ConfigurationFollowRecord) int { return strings.Compare(a.ID, b.ID) })
	return configurationDigest(metadata)
}

func decodeConfigurationRecord(record protocol.ConfigurationFollowRecord, value any) error {
	if len(record.Body) == 0 {
		return invalid("changed configuration record body is required")
	}
	if err := json.Unmarshal(record.Body, value); err != nil {
		return invalid("configuration record body is invalid")
	}
	expected := record.Digest
	record.Digest = ""
	digest, err := configurationDigest(record)
	if err != nil {
		return err
	}
	if digest != expected {
		return invalid("configuration record digest does not match its content")
	}
	return nil
}
