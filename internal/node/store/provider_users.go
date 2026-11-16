package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// readProviderConfiguration admits personal providers and host-provided models.
// Secret use remains internal; settings/export paths additionally require write access.
func readProviderConfiguration(ctx context.Context, db *sql.Conn, id protocol.ProviderID) (*providerConfiguration, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	return scanProviderConfiguration(db.QueryRowContext(ctx, `SELECT body,COALESCE(user,(SELECT id FROM users WHERE local=1)),shared FROM providers WHERE id=?1 AND (COALESCE(user,(SELECT id FROM users WHERE local=1))=?2 OR shared=1)`, id.String(), user.ID.String()))
}

func scanProviderConfiguration(row interface{ Scan(...any) error }) (*providerConfiguration, error) {
	var body []byte
	var owner string
	var shared bool
	if err := row.Scan(&body, &owner, &shared); errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	} else if err != nil {
		return nil, err
	}
	var saved providerConfiguration
	if err := json.Unmarshal(body, &saved); err != nil {
		return nil, err
	}
	ownerID, err := protocol.ParseUserID(owner)
	if err != nil {
		return nil, err
	}
	if saved.Provider != nil {
		saved.Provider.Owner, saved.Provider.Shared = &ownerID, shared
	}
	return &saved, nil
}

func providerConfigurations(ctx context.Context, db *sql.Conn) ([]providerConfiguration, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	rows, err := db.QueryContext(ctx, `SELECT body,COALESCE(user,(SELECT id FROM users WHERE local=1)),shared FROM providers WHERE COALESCE(user,(SELECT id FROM users WHERE local=1))=?1 OR shared=1 ORDER BY rowid`, user.ID.String())
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result := []providerConfiguration{}
	for rows.Next() {
		saved, err := scanProviderConfiguration(rows)
		if err != nil {
			return nil, err
		}
		result = append(result, *saved)
	}
	return result, rows.Err()
}

// Captured provider records are admitted turn state, never request drafts. A
// later visibility change cannot revoke work already admitted with a host model.
func capturedProviderConfiguration(ctx context.Context, db *sql.Conn, captured protocol.ConversationProvider) (*providerConfiguration, error) {
	saved, err := readProviderConfiguration(ctx, db, captured.ID)
	if err != nil || saved != nil || !captured.Shared || captured.Owner == nil {
		return saved, err
	}
	saved, err = scanProviderConfiguration(db.QueryRowContext(ctx, `SELECT body,COALESCE(user,(SELECT id FROM users WHERE local=1)),shared FROM providers WHERE id=?1`, captured.ID.String()))
	if err != nil || saved == nil || saved.Provider == nil || saved.Provider.Owner == nil {
		return nil, err
	}
	if *saved.Provider.Owner != *captured.Owner {
		return nil, fault(protocol.ErrorCodePermissionDenied, "captured provider owner changed")
	}
	return saved, nil
}

func checkProviderWrite(ctx context.Context, db *sql.Conn, id protocol.ProviderID) error {
	if err := CheckConfigurationEditable(ctx, db, "providers", id.String()); err != nil {
		return err
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var owner string
	var shared bool
	err = db.QueryRowContext(ctx, `SELECT COALESCE(user,(SELECT id FROM users WHERE local=1)),shared FROM providers WHERE id=?1`, id.String()).Scan(&owner, &shared)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if shared {
		if !user.Administrator {
			return fault(protocol.ErrorCodePermissionDenied, "host-provided models are managed by an administrator")
		}
		return nil
	}
	if owner != user.ID.String() {
		return fault(protocol.ErrorCodeNotFound, "provider does not exist")
	}
	return nil
}

// ReadProviderSnapshot exposes only the current user's providers and host models.
// Authentication material is absent from both returned public projections.
func ReadProviderSnapshot(ctx context.Context, db *sql.Conn, node protocol.NodeID) ([]protocol.ConversationProvider, []protocol.ConversationQuotaSnapshot, error) {
	saved, err := providerConfigurations(ctx, db)
	if err != nil {
		return nil, nil, err
	}
	values := []protocol.ConversationProvider{}
	quotas := []protocol.ConversationQuotaSnapshot{}
	for _, item := range saved {
		if item.Quota != nil {
			if err := validateProviderQuota(item.ID, *item.Quota); err != nil {
				return nil, nil, err
			}
		}
		if item.Listed && item.Provider != nil {
			values = append(values, *item.Provider)
			if item.Quota != nil && providerQuotaPresent(*item.Quota) && providerQuotaMatches(node, item, *item.Quota) {
				quotas = append(quotas, *item.Quota)
			}
		}
	}
	return values, quotas, nil
}
