package store

import (
	"context"
	"database/sql"
	"errors"
)

func personalSettingKind(kind string) bool {
	switch kind {
	case "media", "compaction", "search", "terminal":
		return true
	}
	return false
}
func readPersonalSetting[T any](ctx context.Context, db *sql.Conn, kind string) (*T, error) {
	if !personalSettingKind(kind) {
		return nil, errors.New("invalid personal setting")
	}
	if registration, followed := personalConfigurationKind(kind); followed {
		if _, cached, err := configurationCacheUser(ctx, db); err != nil {
			return nil, err
		} else if cached {
			return readCachedConfiguration[T](ctx, db, registration.Name, "default")
		}
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	legacy, err := LegacyStorageUser(ctx, db)
	if err != nil {
		return nil, err
	}
	if user.ID == legacy {
		return readBody[T](ctx, db, "SELECT body FROM "+kind+"_settings WHERE singleton=1")
	}
	return readBody[T](ctx, db, "SELECT body FROM user_personal_settings WHERE user=?1 AND kind=?2", user.ID.String(), kind)
}
func writePersonalSetting(ctx context.Context, db *sql.Conn, kind string, body []byte) error {
	if !personalSettingKind(kind) {
		return errors.New("invalid personal setting")
	}
	if registration, followed := personalConfigurationKind(kind); followed {
		if err := CheckConfigurationEditable(ctx, db, registration.Name, "default"); err != nil {
			return err
		}
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	legacy, err := LegacyStorageUser(ctx, db)
	if err != nil {
		return err
	}
	if user.ID == legacy {
		_, err = db.ExecContext(ctx, "INSERT INTO "+kind+"_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO user_personal_settings(user,kind,body) VALUES(?1,?2,?3) ON CONFLICT(user,kind) DO UPDATE SET body=excluded.body", user.ID.String(), kind, body)
	return err
}

// Legacy singleton ownership is fixed to the original local user. Snapshot it
// before merging so a new local administrator keeps its own target settings
// and the source's final values remain readable in the merge archive.
func materializeLegacyPersonalSettings(ctx context.Context, db *sql.Conn) error {
	legacy, err := LegacyStorageUser(ctx, db)
	if err != nil {
		return err
	}
	for _, kind := range []string{"media", "compaction", "search", "terminal"} {
		if _, err := db.ExecContext(ctx, "INSERT INTO user_personal_settings(user,kind,body) SELECT ?1,?2,body FROM "+kind+"_settings WHERE singleton=1 ON CONFLICT(user,kind) DO UPDATE SET body=excluded.body", legacy.String(), kind); err != nil {
			return err
		}
	}
	return nil
}
