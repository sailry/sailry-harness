package store

import (
	"database/sql"
	"errors"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRetiredRoleConfigurationLeavesHistoricalRowsAndLocalNamespace(t *testing.T) {
	storage, _, _, ctx := controllerCacheFixture(t)
	role := protocol.DefaultRoleProfile()
	role.ID, role.Key, role.Name, role.Revision = protocol.NewRoleID(), "reviewer", "Reviewer", 1
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := seedHistoricalConfiguration(ctx, db, "roles", role.ID.String(), 1, role); err != nil {
			return err
		}
		roles, err := ReadRoles(ctx, db)
		if err != nil {
			return err
		}
		if slices.ContainsFunc(roles, func(profile protocol.RoleProfile) bool { return profile.ID == role.ID }) {
			return errors.New("followed role is a current execution choice")
		}
		var formal int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM roles WHERE id=?1", role.ID.String()).Scan(&formal); err != nil {
			return err
		}
		if formal != 0 {
			return errors.New("historical role appeared in authoritative role table")
		}
		role.Name = "Local reviewer"
		if _, err := putRole(ctx, db, role, 0); err != nil {
			return err
		}
		old, err := readConfigurationCacheRecord(ctx, db, "roles", role.ID.String(), 1)
		if err != nil || old == nil {
			return errors.New("local write deleted captured role revision")
		}
		current, err := ReadRoles(ctx, db)
		if err != nil {
			return err
		}
		if !slices.ContainsFunc(current, func(profile protocol.RoleProfile) bool { return profile.ID == role.ID && profile.Name == role.Name }) {
			return errors.New("local role write did not use the execution Node")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
