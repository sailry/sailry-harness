package store

import (
	"context"
	"database/sql"
	"errors"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// initializeUsers assigns legacy peers to the local user; a host-only peer has
// no local resource membership. The surrounding schema transaction is rehearsed
// and backed up before applying this migration to the original database.
func initializeUsers(ctx context.Context, db *sql.Conn) error {
	var present bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM users WHERE local=1)").Scan(&present); err != nil {
		return err
	}
	if !present {
		if _, err := db.ExecContext(ctx, "INSERT INTO users(id,name,administrator,local) VALUES(?1,'Local user',1,1)", protocol.NewUserID().String()); err != nil {
			return err
		}
	}
	_, err := db.ExecContext(ctx, "UPDATE link_peers SET user=(SELECT id FROM users WHERE local=1) WHERE user IS NULL AND direction!='host' AND legacy=1")
	return err
}

func LocalUser(ctx context.Context, db *sql.Conn) (protocol.User, error) {
	return readUser(ctx, db, "SELECT id,name,administrator,local,state,merged_into FROM users WHERE local=1")
}

// CallerUser resolves only authenticated inbound device membership. A known
// outbound host does not become a controller by presenting its device identity.
func CallerUser(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID) (protocol.User, error) {
	if caller == node {
		return LocalUser(ctx, db)
	}
	value, err := readUser(ctx, db, "SELECT u.id,u.name,u.administrator,u.local,u.state,u.merged_into FROM users u JOIN link_peers p ON p.user=u.id WHERE p.identity=?1 AND p.direction IN ('controller','both') AND u.state='active'", caller[:])
	if errors.Is(err, sql.ErrNoRows) {
		return protocol.User{}, fault(protocol.ErrorCodePermissionDenied, "controller access is unavailable")
	}
	return value, err
}

func readUser(ctx context.Context, db *sql.Conn, query string, args ...any) (protocol.User, error) {
	var value protocol.User
	var id string
	var merged sql.NullString
	err := db.QueryRowContext(ctx, query, args...).Scan(&id, &value.Name, &value.Administrator, &value.Local, &value.State, &merged)
	if err != nil {
		return value, err
	}
	value.ID, err = protocol.ParseUserID(id)
	if err != nil {
		return value, err
	}
	if merged.Valid {
		target, err := protocol.ParseUserID(merged.String)
		if err != nil {
			return value, err
		}
		value.MergedInto = &target
	}
	var controller []byte
	if err := db.QueryRowContext(ctx, "SELECT controller FROM users WHERE id=?1", value.ID.String()).Scan(&controller); err != nil {
		return value, err
	}
	if len(controller) != 0 {
		if len(controller) != len(protocol.NodeID{}) {
			return value, errors.New("invalid user controller")
		}
		id := protocol.NodeID(controller)
		value.Controller = &id
	}
	return value, nil
}

// CurrentController resolves the authority for personal configuration. A local
// or legacy user without a recorded controller keeps this Node as authority.
func CurrentController(ctx context.Context, db *sql.Conn, node protocol.NodeID) (protocol.NodeID, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return protocol.NodeID{}, err
	}
	if user.Controller != nil {
		return *user.Controller, nil
	}
	return node, nil
}

func ListUsers(ctx context.Context, db *sql.Conn) ([]protocol.User, error) {
	rows, err := db.QueryContext(ctx, "SELECT id FROM users ORDER BY local DESC,rowid")
	if err != nil {
		return nil, err
	}
	var ids []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			rows.Close()
			return nil, err
		}
		ids = append(ids, id)
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	values := make([]protocol.User, 0, len(ids))
	for _, id := range ids {
		value, err := readUser(ctx, db, "SELECT id,name,administrator,local,state,merged_into FROM users WHERE id=?1", id)
		if err != nil {
			return nil, err
		}
		values = append(values, value)
	}
	return values, nil
}

func CreateUser(ctx context.Context, db *sql.Conn, name string) (protocol.User, error) {
	name = strings.TrimSpace(name)
	if name == "" || len([]rune(name)) > 128 || strings.ContainsFunc(name, unicode.IsControl) {
		return protocol.User{}, invalid("invalid user name")
	}
	var first bool
	if err := db.QueryRowContext(ctx, "SELECT NOT EXISTS(SELECT 1 FROM users WHERE local=0)").Scan(&first); err != nil {
		return protocol.User{}, err
	}
	value := protocol.User{ID: protocol.NewUserID(), Name: name, Administrator: first, State: "active"}
	_, err := db.ExecContext(ctx, "INSERT INTO users(id,name,administrator) VALUES(?1,?2,?3)", value.ID.String(), value.Name, value.Administrator)
	return value, err
}

func SetUserAdministrator(ctx context.Context, db *sql.Conn, id protocol.UserID, administrator bool) error {
	result, err := db.ExecContext(ctx, "UPDATE users SET administrator=?2 WHERE id=?1 AND state='active' AND (local=0 OR ?2=1)", id.String(), administrator)
	if err != nil {
		return err
	}
	count, err := result.RowsAffected()
	if err == nil && count == 0 {
		return fault(protocol.ErrorCodeNotFound, "user is unavailable")
	}
	return err
}

func validateUsers(ctx context.Context, db *sql.Conn) error {
	users, err := ListUsers(ctx, db)
	if err != nil {
		return err
	}
	local := 0
	for _, user := range users {
		if user.Local {
			local++
			if !user.Administrator || user.State != "active" {
				return errors.New("invalid local user")
			}
		}
		if user.State == "merged" && user.MergedInto == nil || user.State == "active" && user.MergedInto != nil {
			return errors.New("invalid merged user")
		}
	}
	if local != 1 {
		return errors.New("local user is missing")
	}
	var invalid bool
	err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM link_peers WHERE direction!='host' AND user IS NULL)").Scan(&invalid)
	if err == nil && invalid {
		return errors.New("controller user is missing")
	}
	return err
}
