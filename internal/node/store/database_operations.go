package store

import (
	"context"
	"database/sql"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// DatabaseOperation is everything an external database operation needs after
// the storage lease is released. It carries secrets: never log, encode or
// return it to a controller.
type DatabaseOperation struct {
	Profile  protocol.DatabaseProfile
	Password protocol.Secret
	// Ssh is set only for an SSH-tunnelled profile.
	Ssh   *DatabaseTunnel
	Query *DatabaseQuery
	// Browse is set only for browse_database.
	Browse *DatabaseBrowse
	// Storage is the Node profile directory external SQLite must not access.
	Storage string
}

type DatabaseTunnel struct {
	Profile    protocol.SshProfile
	Credential protocol.SshCredential
}

type DatabaseQuery struct {
	Sql       string
	RowLimit  int
	TimeoutMs uint64
}

type DatabaseBrowse struct {
	Database *string
}

// PrepareDatabase validates test_database, check_database, browse_database or
// query_database against the saved profile and reads its secret and tunnel
// material. Execute the returned operation outside the storage lease.
func PrepareDatabase(ctx context.Context, db *sql.Conn, storage string, command protocol.Command) (DatabaseOperation, error) {
	operation := DatabaseOperation{Storage: storage}
	var id protocol.DatabaseID
	var revision uint64
	var selected *string
	readOnly := false
	switch command.Kind {
	case "test_database":
		input, valid := dispatchValue[protocol.CommandTestDatabasePayload](command.Data)
		if !valid {
			return operation, invalid("database operation expected")
		}
		return prepareDatabaseTest(ctx, db, operation, input)
	case "check_database":
		input, valid := dispatchValue[protocol.CommandCheckDatabasePayload](command.Data)
		if !valid {
			return operation, invalid("database operation expected")
		}
		id, revision = input.Profile, input.ExpectedRevision
	case "browse_database":
		input, valid := dispatchValue[protocol.CommandBrowseDatabasePayload](command.Data)
		if !valid {
			return operation, invalid("database operation expected")
		}
		id, revision, selected = input.Profile, input.ExpectedRevision, input.Database
		operation.Browse = &DatabaseBrowse{Database: input.Database}
	case "query_database":
		input, valid := dispatchValue[protocol.CommandQueryDatabasePayload](command.Data)
		if !valid {
			return operation, invalid("database operation expected")
		}
		if strings.TrimSpace(input.Sql) == "" || len(input.Sql) > 65536 || input.RowLimit < 1 || input.RowLimit > 10_000 ||
			input.TimeoutMs < 1 || input.TimeoutMs > 3_600_000 {
			return operation, invalid("invalid database query or limit")
		}
		id, revision, selected, readOnly = input.Profile, input.ExpectedRevision, input.Database, input.ReadOnly
		operation.Query = &DatabaseQuery{Sql: input.Sql, RowLimit: int(input.RowLimit), TimeoutMs: input.TimeoutMs}
	default:
		return operation, invalid("database operation expected")
	}
	profile, err := currentDatabase(ctx, db, id, revision)
	if err != nil {
		return operation, err
	}
	if readOnly {
		profile.ReadOnly = true
	}
	if profile.Connection.Kind != "sqlite" {
		if err := checkDatabaseCredential(ctx, db, id, profile.Connection); err != nil {
			return operation, err
		}
	}
	if selected != nil {
		if *selected == "" || len(*selected) > 256 || strings.ContainsFunc(*selected, unicode.IsControl) {
			return operation, invalid("invalid selected database")
		}
		if profile.Connection, err = selectDatabase(profile.Connection, *selected); err != nil {
			return operation, err
		}
	}
	var password string
	if err := db.QueryRowContext(ctx, "SELECT password FROM database_profiles WHERE id=?1", id.String()).Scan(&password); err != nil {
		return operation, err
	}
	operation.Profile, operation.Password = profile, protocol.NewSecret(password)
	operation.Ssh, err = prepareDatabaseTunnel(ctx, db, profile.Connection)
	return operation, err
}

// prepareDatabaseTest checks an unsaved draft. Without a supplied password it
// may reuse the saved one only for the saved target.
func prepareDatabaseTest(ctx context.Context, db *sql.Conn, operation DatabaseOperation, input protocol.CommandTestDatabasePayload) (DatabaseOperation, error) {
	previous, err := ReadDatabase(ctx, db, input.Profile.ID)
	if err != nil {
		return operation, err
	}
	current := uint64(0)
	if previous != nil {
		current = previous.Revision
	}
	if err := checkRevision(current, input.ExpectedRevision); err != nil {
		return operation, err
	}
	if err := validateDatabaseConnection(input.Profile.Connection, input.Password); err != nil {
		return operation, err
	}
	switch {
	case input.Password != nil:
		operation.Password = *input.Password
	case previous != nil && input.Profile.Connection.Kind != "sqlite":
		if err := checkDatabaseCredential(ctx, db, input.Profile.ID, input.Profile.Connection); err != nil {
			return operation, err
		}
		var password string
		if err := db.QueryRowContext(ctx, "SELECT password FROM database_profiles WHERE id=?1", input.Profile.ID.String()).Scan(&password); err != nil {
			return operation, err
		}
		operation.Password = protocol.NewSecret(password)
	}
	operation.Profile = input.Profile
	operation.Ssh, err = prepareDatabaseTunnel(ctx, db, input.Profile.Connection)
	return operation, err
}

// selectDatabase replaces the server database of a copy of the connection.
// SQLite exposes only its main database.
func selectDatabase(connection protocol.DatabaseConnection, selected string) (protocol.DatabaseConnection, error) {
	switch connection.Kind {
	case "network":
		if target, valid := dispatchValue[protocol.DatabaseConnectionNetworkPayload](connection.Data); valid {
			target.Database = selected
			connection.Data = &target
			return connection, nil
		}
	case "socket":
		if target, valid := dispatchValue[protocol.DatabaseConnectionSocketPayload](connection.Data); valid {
			target.Database = selected
			connection.Data = &target
			return connection, nil
		}
	case "ssh":
		if target, valid := dispatchValue[protocol.DatabaseConnectionSshPayload](connection.Data); valid {
			target.Database = selected
			connection.Data = &target
			return connection, nil
		}
	case "sqlite":
		if selected != "main" {
			return connection, invalid("unknown SQLite database")
		}
		return connection, nil
	}
	return connection, invalid("invalid database connection")
}

func prepareDatabaseTunnel(ctx context.Context, db *sql.Conn, connection protocol.DatabaseConnection) (*DatabaseTunnel, error) {
	saved, err := databaseTunnelProfile(ctx, db, connection)
	if err != nil || saved == nil {
		return nil, err
	}
	profile, credential, err := ReadSsh(ctx, db, saved.ID, saved.Revision)
	if err != nil {
		return nil, err
	}
	return &DatabaseTunnel{Profile: profile, Credential: credential}, nil
}
