package store

import (
	"context"
	"database/sql"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ApplyConfigurationManifest updates retained model credentials for admitted
// captures. Other configuration is owned by the execution Node.
func ApplyConfigurationManifest(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, manifest protocol.ConfigurationManifest) ([]protocol.Event, error) {
	if err := validateConfigurationManifest(manifest); err != nil {
		return nil, err
	}
	if _, err := configurationReceiver(ctx, db, node, caller, manifest.Source); err != nil {
		return nil, err
	}
	if err := checkRetainedConfigurationManifest(manifest); err != nil {
		return nil, err
	}
	return applyConfigurationCache(ctx, db, node, caller, manifest)
}

func checkRetainedConfigurationManifest(manifest protocol.ConfigurationManifest) error {
	if manifest.Category != "runtime_credentials" {
		return fault(protocol.ErrorCodeNotConfigured, "configuration is managed on its owning Node")
	}
	for _, record := range manifest.Records {
		if !strings.HasPrefix(record.ID, "provider:") {
			return fault(protocol.ErrorCodeNotConfigured, "connector credentials are managed on their owning Node")
		}
	}
	return nil
}

func configurationReceiver(ctx context.Context, db *sql.Conn, node, caller, source protocol.NodeID) (protocol.User, error) {
	if source != caller || caller == node {
		return protocol.User{}, fault(protocol.ErrorCodePermissionDenied, "configuration source does not match authenticated caller")
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return user, err
	}
	member, err := CallerUser(ctx, db, node, caller)
	if err != nil || member.ID != user.ID {
		return user, fault(protocol.ErrorCodePermissionDenied, "configuration source is not a member of this user")
	}
	if user.Local {
		return user, fault(protocol.ErrorCodePermissionDenied, "host-local configuration is not a controller replica")
	}
	if user.Controller == nil || *user.Controller != caller {
		return user, fault(protocol.ErrorCodePermissionDenied, "configuration belongs to a different controller")
	}
	return user, nil
}
