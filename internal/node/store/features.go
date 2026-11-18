package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
func fault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func checkRevision(actual, expected uint64) error {
	if actual != expected {
		return fault(protocol.ErrorCodeRevisionConflict, "configuration revision changed; reload before editing")
	}
	return nil
}

func nextRevision(revision uint64) (uint64, error) {
	if revision >= math.MaxInt64 {
		return 0, invalid("configuration revision exhausted")
	}
	return revision + 1, nil
}

func encode(value any) ([]byte, error) { return json.Marshal(value) }

func readBody[T any](ctx context.Context, db *sql.Conn, query string, args ...any) (*T, error) {
	var body []byte
	err := db.QueryRowContext(ctx, query, args...).Scan(&body)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var value T
	if err := json.Unmarshal(body, &value); err != nil {
		return nil, err
	}
	return &value, nil
}

func readBodies[T any](ctx context.Context, db *sql.Conn, query string, args ...any) ([]T, error) {
	rows, err := db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	values := []T{}
	for rows.Next() {
		var body []byte
		if err := rows.Scan(&body); err != nil {
			return nil, err
		}
		var value T
		if err := json.Unmarshal(body, &value); err != nil {
			return nil, err
		}
		values = append(values, value)
	}
	return values, rows.Err()
}

func validateConfig(ctx context.Context, db *sql.Conn, node protocol.NodeID, config protocol.SessionConfig) error {
	if strings.TrimSpace(config.Model) == "" || len(config.Model) > 256 {
		return invalid("model identifier is required and must be bounded")
	}
	stored, err := readProviderConfiguration(ctx, db, config.Provider)
	if err != nil {
		return err
	}
	if stored == nil || stored.Provider == nil {
		return protocol.NewConfigurationFault(protocol.ConfigurationProviderMissing, "model channel is not configured on this Node")
	}
	if !stored.Listed {
		return protocol.NewConfigurationFault(protocol.ConfigurationProviderRemoved, "model channel is not available on this Node")
	}
	if ref := config.Credential; ref != nil {
		if _, err := resolveProviderKey(ctx, db, node, *ref, config.Provider); err != nil {
			// A captured runtime account can wait at its next unsent model
			// step; expiry does not invalidate the selected configuration.
			if stored.Provider == nil || stored.Provider.Authentication == protocol.AuthenticationApiKey || !expiredRuntimeCredential(err) {
				return err
			}
		}
	}
	if err := providers.ValidateSelection(*stored.Provider, config); err != nil {
		return err
	}
	return nil
}

// Older direct sessions could capture a remote credential without a profile.
func hasRemoteModelCapture(session protocol.Session, node protocol.NodeID) bool {
	return session.Profile != nil && session.Profile.Source != node ||
		session.Config.Credential != nil && session.Config.Credential.Node != node
}

func validateCapturedConfig(ctx context.Context, db *sql.Conn, node protocol.NodeID, profile protocol.SessionProfile, config protocol.SessionConfig) error {
	provider := profile.Provider
	if provider.ID != config.Provider {
		return fault(protocol.ErrorCodePermissionDenied, "captured model channel identity changed")
	}
	if (provider.Credential == nil) != (config.Credential == nil) || provider.Credential != nil && *provider.Credential != *config.Credential {
		return fault(protocol.ErrorCodePermissionDenied, "captured model credential identity changed")
	}
	if config.Credential != nil {
		if profile.Source == node {
			if config.Credential.Node != node {
				return fault(protocol.ErrorCodePermissionDenied, "captured model credential belongs to another Node")
			}
			if _, err := resolveCapturedProviderKey(ctx, db, node, *config.Credential, provider); err != nil {
				return err
			}
		} else {
			if config.Credential.Node != profile.Source {
				return fault(protocol.ErrorCodePermissionDenied, "captured model credential belongs to another Node")
			}
			if _, err := ReadRuntimeProviderCredential(ctx, db, node, provider, *config.Credential); err != nil &&
				(provider.Authentication == protocol.AuthenticationApiKey || !expiredRuntimeCredential(err)) {
				return err
			}
		}
	}
	return providers.ValidateSelection(provider, config)
}

// Preserve retired private fields exactly as the current v1 configuration does.
func preserveConfig(previous []byte, config protocol.SessionConfig) ([]byte, error) {
	next, err := encode(config)
	if err != nil || len(previous) == 0 {
		return next, err
	}
	var before, known, after map[string]json.RawMessage
	if err := json.Unmarshal(previous, &before); err != nil {
		return nil, err
	}
	var public protocol.SessionConfig
	if err := json.Unmarshal(previous, &public); err != nil {
		return nil, err
	}
	body, err := encode(public)
	if err != nil {
		return nil, err
	}
	if err := json.Unmarshal(body, &known); err != nil {
		return nil, err
	}
	if err := json.Unmarshal(next, &after); err != nil {
		return nil, err
	}
	for key, value := range before {
		if _, exists := known[key]; !exists {
			if _, exists := after[key]; !exists {
				after[key] = value
			}
		}
	}
	return encode(after)
}

// providerConfiguration reads only the existing inline storage representation.
// Authentication data never enters a controller snapshot or event.
type providerConfiguration struct {
	ID             protocol.ProviderID                             `json:"id"`
	Provider       *protocol.ConversationProvider                  `json:"provider"`
	Listed         bool                                            `json:"listed"`
	Authorizations map[protocol.CredentialID]providerAuthorization `json:"authorizations"`
	Quota          *protocol.ConversationQuotaSnapshot             `json:"quota"`
}

type providerAuthorization struct {
	Authentication protocol.Authentication  `json:"authentication"`
	Revision       uint64                   `json:"revision"`
	Key            *protocol.Secret         `json:"key"`
	ExpiresAtMs    *uint64                  `json:"expires_at_ms"`
	Auth           *providers.AccountTarget `json:"auth,omitempty"`
}

func ReadDefaults(ctx context.Context, db *sql.Conn) (protocol.Defaults, error) {
	var value protocol.Defaults
	var body []byte
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return value, err
	}
	if user.Local {
		err = db.QueryRowContext(ctx, "SELECT revision,config FROM defaults WHERE singleton=1").Scan(&value.Revision, &body)
	} else {
		err = db.QueryRowContext(ctx, "SELECT revision,config FROM user_defaults WHERE user=?1", user.ID.String()).Scan(&value.Revision, &body)
	}
	if errors.Is(err, sql.ErrNoRows) {
		return protocol.DefaultDefaults(), nil
	}
	if err != nil {
		return value, err
	}
	var config protocol.SessionConfig
	if err := json.Unmarshal(body, &config); err != nil {
		return value, err
	}
	value.Config = &config
	return value, nil
}

// SaveDefaults replaces only the active user's default session configuration.
func SaveDefaults(ctx context.Context, db *sql.Conn, node protocol.NodeID, input protocol.Defaults) (protocol.Defaults, error) {
	current, err := ReadDefaults(ctx, db)
	if err != nil {
		return protocol.Defaults{}, err
	}
	if err := checkRevision(current.Revision, input.Revision); err != nil {
		return protocol.Defaults{}, err
	}
	if input.Config == nil {
		return protocol.Defaults{}, invalid("default session configuration is required")
	}
	saved, err := readProviderConfiguration(ctx, db, input.Config.Provider)
	if err != nil {
		return protocol.Defaults{}, err
	}
	if saved == nil || !saved.Listed || saved.Provider == nil {
		return protocol.Defaults{}, fault(protocol.ErrorCodeNotFound, "default provider does not exist")
	}
	if err := validateConfig(ctx, db, node, *input.Config); err != nil {
		return protocol.Defaults{}, err
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return protocol.Defaults{}, err
	}
	input.Revision, err = nextRevision(current.Revision)
	if err != nil {
		return protocol.Defaults{}, err
	}
	body, err := encode(input.Config)
	if err != nil {
		return protocol.Defaults{}, err
	}
	if user.Local {
		_, err = db.ExecContext(ctx, "INSERT INTO defaults(singleton,revision,config) VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision,config=excluded.config", input.Revision, body)
	} else {
		_, err = db.ExecContext(ctx, "INSERT INTO user_defaults(user,revision,config) VALUES(?1,?2,?3) ON CONFLICT(user) DO UPDATE SET revision=excluded.revision,config=excluded.config", user.ID.String(), input.Revision, body)
	}
	return input, err
}
