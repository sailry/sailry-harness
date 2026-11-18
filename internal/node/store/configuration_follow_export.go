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
func ExportConfigurationManifest(ctx context.Context, db *sql.Conn, node, host protocol.NodeID, category string, secrets bool, declarations map[string]protocol.PluginMcpConfiguration, known *protocol.ConfigurationManifest) (protocol.ConfigurationManifest, error) {
	value := protocol.ConfigurationManifest{Source: node, Category: category, Records: []protocol.ConfigurationFollowRecord{}}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return value, err
	}
	if !user.Local {
		return value, fault(protocol.ErrorCodePermissionDenied, "only local configuration can be published")
	}
	kind, ok := configurationKind(category)
	if !ok || kind.Export == nil || kind.LocalOnly {
		return value, invalid("unsupported configuration category")
	}
	options := ConfigurationExport{Node: node, Secrets: secrets, Declarations: declarations}
	if category == "runtime_credentials" && known != nil && len(known.Records) != 0 {
		options.Captured, err = authorizedRuntimeCaptures(ctx, db, user, host, node, *known)
		if err != nil {
			return value, err
		}
	}
	value.Records, err = kind.Export(ctx, db, options)
	if err != nil {
		return value, err
	}
	digest, err := configurationManifestDigest(value.Records)
	if err != nil {
		return value, err
	}
	slices.SortFunc(value.Records, func(a, b protocol.ConfigurationFollowRecord) int { return strings.Compare(a.ID, b.ID) })
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
