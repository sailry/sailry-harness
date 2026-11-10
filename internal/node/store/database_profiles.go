package store

import (
	"context"
	"database/sql"
	"math"
	"path/filepath"
	"runtime"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const databaseProfileCapacity = 128

func ListDatabases(ctx context.Context, db *sql.Conn) ([]protocol.DatabaseProfile, error) {
	return readBodies[protocol.DatabaseProfile](ctx, db, "SELECT body FROM database_profiles ORDER BY id")
}

// ReadDatabase returns nil when the profile is not saved on this Node.
func ReadDatabase(ctx context.Context, db *sql.Conn, id protocol.DatabaseID) (*protocol.DatabaseProfile, error) {
	return readBody[protocol.DatabaseProfile](ctx, db, "SELECT body FROM database_profiles WHERE id=?1", id.String())
}

func currentDatabase(ctx context.Context, db *sql.Conn, id protocol.DatabaseID, revision uint64) (protocol.DatabaseProfile, error) {
	profile, err := ReadDatabase(ctx, db, id)
	if err != nil {
		return protocol.DatabaseProfile{}, err
	}
	if profile == nil {
		return protocol.DatabaseProfile{}, fault(protocol.ErrorCodeNotFound, "database profile does not exist on this Node")
	}
	return *profile, checkRevision(profile.Revision, revision)
}

// ExecuteDatabaseCommand owns the profile configuration commands. Passwords
// stay in their own column and never enter an output, event or fault.
func ExecuteDatabaseCommand(ctx context.Context, db *sql.Conn, command protocol.Command) (protocol.Output, []protocol.Event, error) {
	switch command.Kind {
	case "list_databases":
		profiles, err := ListDatabases(ctx, db)
		return protocol.Output{Kind: "database_profiles", Data: profiles}, nil, err
	case "save_database":
		input, valid := dispatchValue[protocol.CommandSaveDatabasePayload](command.Data)
		if !valid {
			break
		}
		return saveDatabase(ctx, db, input)
	case "remove_database":
		input, valid := dispatchValue[protocol.CommandRemoveDatabasePayload](command.Data)
		if !valid {
			break
		}
		if _, err := currentDatabase(ctx, db, input.Profile, input.ExpectedRevision); err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM database_profiles WHERE id=?1", input.Profile.String()); err != nil {
			return protocol.Output{}, nil, err
		}
		profiles, err := ListDatabases(ctx, db)
		return protocol.Output{Kind: "database_profiles", Data: profiles}, []protocol.Event{{Kind: "database_removed", Data: protocol.EventDatabaseRemovedPayload{ID: input.Profile}}}, err
	}
	return protocol.Output{}, nil, invalid("database configuration command expected")
}

func saveDatabase(ctx context.Context, db *sql.Conn, input protocol.CommandSaveDatabasePayload) (protocol.Output, []protocol.Event, error) {
	previous, err := ReadDatabase(ctx, db, input.Profile.ID)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	current := uint64(0)
	if previous != nil {
		current = previous.Revision
	}
	if err := checkRevision(current, input.ExpectedRevision); err != nil {
		return protocol.Output{}, nil, err
	}
	if err := validateSshSharing(ctx, db, input.Profile.Sharing); err != nil {
		return protocol.Output{}, nil, err
	}
	profile := input.Profile
	profile.Name = strings.TrimSpace(profile.Name)
	if profile.Name == "" || len(profile.Name) > 256 {
		return protocol.Output{}, nil, invalid("invalid database profile name")
	}
	if err := validateDatabaseConnection(profile.Connection, input.Password); err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err := databaseTunnelProfile(ctx, db, profile.Connection); err != nil {
		return protocol.Output{}, nil, err
	}
	if input.ExpectedRevision >= math.MaxInt64 {
		return protocol.Output{}, nil, invalid("database profile revision exhausted")
	}
	profile.Revision = input.ExpectedRevision + 1
	sqlite := profile.Connection.Kind == "sqlite"
	if previous != nil && input.Password == nil && !sqlite {
		if err := checkDatabaseCredential(ctx, db, profile.ID, profile.Connection); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	target, err := captureDatabaseCredential(ctx, db, profile.Connection)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	// A nil password keeps the stored one; an empty one clears it.
	var password any
	if sqlite {
		password = ""
	} else if input.Password != nil {
		password = input.Password.Expose()
	}
	body, err := encode(profile)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if previous == nil {
		var count int
		if err := db.QueryRowContext(ctx, "SELECT COUNT(*) FROM database_profiles").Scan(&count); err != nil {
			return protocol.Output{}, nil, err
		}
		if count >= databaseProfileCapacity {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeBusy, "database profile capacity exhausted")
		}
		if password == nil {
			password = ""
		}
		_, err = db.ExecContext(ctx, "INSERT INTO database_profiles(id,body,password,password_target) VALUES(?1,?2,?3,?4)", profile.ID.String(), body, password, target)
	} else {
		_, err = db.ExecContext(ctx, "UPDATE database_profiles SET body=?2,password=COALESCE(?3,password),password_target=?4 WHERE id=?1", profile.ID.String(), body, password, target)
	}
	if err != nil {
		return protocol.Output{}, nil, err
	}
	return protocol.Output{Kind: "database_profile", Data: profile}, []protocol.Event{{Kind: "database_changed", Data: profile}}, nil
}

func validateDatabaseConnection(connection protocol.DatabaseConnection, password *protocol.Secret) error {
	switch connection.Kind {
	case "sqlite":
		target, valid := dispatchValue[protocol.DatabaseConnectionSqlitePayload](connection.Data)
		if !valid {
			break
		}
		if !databaseNodePath(target.Path) {
			return invalid("SQLite requires an absolute path on the execution Node")
		}
		if password != nil {
			return invalid("SQLite does not use a network password")
		}
		return nil
	case "network":
		target, valid := dispatchValue[protocol.DatabaseConnectionNetworkPayload](connection.Data)
		if !valid {
			break
		}
		if !databaseHost(target.Host) {
			return invalid("invalid database host")
		}
		return validateDatabaseNetwork(target.Engine, target.Port, target.Database, target.Username, password)
	case "ssh":
		target, valid := dispatchValue[protocol.DatabaseConnectionSshPayload](connection.Data)
		if !valid {
			break
		}
		if !databaseHost(target.Host) {
			return invalid("invalid database host")
		}
		return validateDatabaseNetwork(target.Engine, target.Port, target.Database, target.Username, password)
	case "socket":
		target, valid := dispatchValue[protocol.DatabaseConnectionSocketPayload](connection.Data)
		if !valid {
			break
		}
		if runtime.GOOS == "windows" {
			return fault(protocol.ErrorCodeUnavailable, "Database sockets are unavailable on this execution Node")
		}
		if !databaseNodePath(target.Path) {
			return invalid("Socket requires an absolute path on the execution Node")
		}
		return validateDatabaseNetwork(target.Engine, target.Port, target.Database, target.Username, password)
	}
	return invalid("invalid database connection")
}

func databaseNodePath(path string) bool {
	return len(path) <= 4096 && !strings.ContainsRune(path, 0) && filepath.IsAbs(path)
}

func databaseHost(host string) bool {
	return host != "" && len(host) <= 253 && !strings.Contains(host, "/") &&
		!strings.ContainsFunc(host, func(char rune) bool { return unicode.IsSpace(char) || unicode.IsControl(char) })
}

func validateDatabaseNetwork(engine protocol.DatabaseEngine, port uint16, database, username string, password *protocol.Secret) error {
	if engine != protocol.DatabaseEngineMysql && engine != protocol.DatabaseEnginePostgres || port == 0 || len(database) > 256 ||
		username == "" || len(username) > 256 || strings.ContainsFunc(username, unicode.IsControl) ||
		strings.ContainsFunc(database, unicode.IsControl) || password != nil && len(password.Expose()) > 65536 {
		return invalid("invalid network database profile")
	}
	return nil
}

// databaseTunnelProfile returns the saved SSH profile an SSH-tunnelled
// connection names, or nil for every other connection.
func databaseTunnelProfile(ctx context.Context, db *sql.Conn, connection protocol.DatabaseConnection) (*protocol.SshProfile, error) {
	if connection.Kind != "ssh" {
		return nil, nil
	}
	target, valid := dispatchValue[protocol.DatabaseConnectionSshPayload](connection.Data)
	if !valid {
		return nil, invalid("invalid database connection")
	}
	profile, err := readBody[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles WHERE id=?1", target.Ssh.String())
	if err == nil && profile == nil {
		err = fault(protocol.ErrorCodeNotFound, "SSH connection does not exist on this Node")
	}
	return profile, err
}
