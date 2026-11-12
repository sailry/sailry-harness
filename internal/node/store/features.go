package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"strings"
	"time"

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
	if ref := config.Credential; ref != nil {
		if ref.Node != node {
			return fault(protocol.ErrorCodePermissionDenied, "credential references must belong to the execution Node")
		}
		stored, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE json_type(body,?1) IS NOT NULL", `$.authorizations."`+ref.ID.String()+`"`)
		if err != nil {
			return err
		}
		if stored == nil {
			return fault(protocol.ErrorCodeNotFound, "credential not found on the execution Node")
		}
		saved, exists := stored.Authorizations[ref.ID]
		if !exists {
			return fault(protocol.ErrorCodeNotFound, "credential not found on the execution Node")
		}
		if stored.ID != config.Provider || saved.Key == nil {
			return fault(protocol.ErrorCodePermissionDenied, "credential is revoked or belongs to another provider")
		}
		if saved.ExpiresAtMs != nil && *saved.ExpiresAtMs <= uint64(time.Now().UnixMilli()) {
			return fault(protocol.ErrorCodePermissionDenied, "credential expired; authenticate again")
		}
	}
	stored, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", config.Provider.String())
	if err != nil {
		return err
	}
	if stored != nil && stored.Provider != nil {
		if err := providers.ValidateSelection(*stored.Provider, config); err != nil {
			return err
		}
	} else if len(config.Features) != 0 {
		return invalid("model feature choices require an advertised model")
	}
	return nil
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
	err := db.QueryRowContext(ctx, "SELECT revision,config FROM defaults WHERE singleton=1").Scan(&value.Revision, &body)
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
