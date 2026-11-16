package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// InitializeConfigurationUsers runs after the local user exists in the same
// rehearsed schema transaction. Legacy configuration remains private to it.
func InitializeConfigurationUsers(ctx context.Context, db *sql.Conn) error {
	for _, table := range []string{"providers", "roles", "teams", "ssh_profiles", "database_profiles"} {
		if _, err := db.ExecContext(ctx, "UPDATE "+table+" SET user=(SELECT id FROM users WHERE local=1) WHERE user IS NULL"); err != nil {
			return err
		}
	}
	return nil
}

func configurationTable(table string) bool {
	switch table {
	case "roles", "teams", "ssh_profiles", "database_profiles":
		return true
	}
	return false
}

func configurationOwner(ctx context.Context, db *sql.Conn, table, id string) (protocol.UserID, bool, error) {
	if !configurationTable(table) && table != "providers" {
		return protocol.UserID{}, false, errors.New("invalid configuration table")
	}
	var owner string
	err := db.QueryRowContext(ctx, "SELECT COALESCE(user,(SELECT id FROM users WHERE local=1)) FROM "+table+" WHERE id=?1", id).Scan(&owner)
	if errors.Is(err, sql.ErrNoRows) {
		return protocol.UserID{}, false, nil
	}
	if err != nil {
		return protocol.UserID{}, false, err
	}
	value, err := protocol.ParseUserID(owner)
	return value, true, err
}

func checkConfigurationOwner(ctx context.Context, db *sql.Conn, table, id string) error {
	if err := CheckConfigurationEditable(ctx, db, table, id); err != nil {
		return err
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	owner, present, err := configurationOwner(ctx, db, table, id)
	if err != nil {
		return err
	}
	if present && owner != user.ID {
		return fault(protocol.ErrorCodeNotFound, "configuration is unavailable")
	}
	return nil
}

func readUserConfiguration[T any](ctx context.Context, db *sql.Conn, table, id string) (*T, error) {
	if !configurationTable(table) {
		return nil, errors.New("invalid configuration table")
	}
	if _, cached, err := configurationCacheUser(ctx, db); err != nil {
		return nil, err
	} else if cached {
		return readCachedConfiguration[T](ctx, db, table, id)
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	return readBody[T](ctx, db, "SELECT body FROM "+table+" WHERE id=?1 AND COALESCE(user,(SELECT id FROM users WHERE local=1))=?2", id, user.ID.String())
}

func listUserConfiguration[T any](ctx context.Context, db *sql.Conn, table string) ([]T, error) {
	if !configurationTable(table) {
		return nil, errors.New("invalid configuration table")
	}
	if _, cached, err := configurationCacheUser(ctx, db); err != nil {
		return nil, err
	} else if cached {
		return listCachedConfiguration[T](ctx, db, table)
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	order := "id"
	if table == "roles" || table == "teams" {
		order = "key"
	}
	return readBodies[T](ctx, db, "SELECT body FROM "+table+" WHERE COALESCE(user,(SELECT id FROM users WHERE local=1))=?1 ORDER BY "+order, user.ID.String())
}

// Keys in the JSON contracts remain ordinary role/team keys. Only their index
// representation includes the user, preserving the existing UNIQUE constraint.
func configurationKey(user protocol.User, key string) string {
	if user.Local {
		return key
	}
	return user.ID.String() + ":" + key
}

func validateConfigurationUsers(ctx context.Context, db *sql.Conn) error {
	for _, table := range []string{"providers", "roles", "teams", "ssh_profiles", "database_profiles"} {
		var invalid bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM "+table+" c LEFT JOIN users u ON u.id=COALESCE(c.user,(SELECT id FROM users WHERE local=1)) WHERE u.id IS NULL)").Scan(&invalid); err != nil {
			return err
		}
		if invalid {
			return errors.New("configuration owner is unavailable")
		}
	}
	for _, table := range []string{"roles", "teams"} {
		var invalid bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM "+table+" c JOIN users u ON u.id=COALESCE(c.user,(SELECT id FROM users WHERE local=1)) WHERE c.key!=CASE WHEN u.local=1 THEN json_extract(c.body,'$.key') ELSE u.id||':'||json_extract(c.body,'$.key') END)").Scan(&invalid); err != nil {
			return err
		}
		if invalid {
			return errors.New("configuration key does not match its owner")
		}
	}
	return nil
}
