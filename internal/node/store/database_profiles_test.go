package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func databaseFixtureNetwork(host string) protocol.DatabaseConnection {
	return protocol.DatabaseConnection{Kind: "network", Data: &protocol.DatabaseConnectionNetworkPayload{
		Engine: protocol.DatabaseEnginePostgres, Host: host, Port: 5432, Database: "app", Username: "reader", Tls: protocol.DatabaseTlsRequire,
	}}
}

func databaseFixtureTunnel(tunnel protocol.SshID) protocol.DatabaseConnection {
	return protocol.DatabaseConnection{Kind: "ssh", Data: &protocol.DatabaseConnectionSshPayload{
		Engine: protocol.DatabaseEngineMysql, Host: "db.internal", Port: 3306, Database: "app", Username: "reader", Tls: protocol.DatabaseTlsPrefer, Ssh: tunnel,
	}}
}

func databaseFixtureSqlite(path string) protocol.DatabaseConnection {
	return protocol.DatabaseConnection{Kind: "sqlite", Data: &protocol.DatabaseConnectionSqlitePayload{Path: path}}
}

func databaseFixtureSave(ctx context.Context, db *sql.Conn, profile protocol.DatabaseProfile, expected uint64, password *string) (protocol.Output, []protocol.Event, error) {
	input := &protocol.CommandSaveDatabasePayload{Profile: profile, ExpectedRevision: expected}
	if password != nil {
		secret := protocol.NewSecret(*password)
		input.Password = &secret
	}
	return ExecuteDatabaseCommand(ctx, db, protocol.Command{Kind: "save_database", Data: input})
}

func databaseFixtureSsh(ctx context.Context, db *sql.Conn, profile protocol.SshProfile, expected uint64, password *string) (protocol.SshProfile, error) {
	input := &protocol.CommandSaveSshPayload{Profile: profile, ExpectedRevision: expected}
	if password != nil {
		input.Credential = &protocol.SshCredential{Kind: "password", Data: &protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(*password)}}
	}
	output, _, err := ExecuteSshCommand(ctx, db, protocol.Command{Kind: "save_ssh", Data: input})
	if err != nil {
		return profile, err
	}
	return output.Data.(protocol.SshProfile), nil
}

// databaseFixturePublic fails when any secret reaches an output, event or fault.
func databaseFixturePublic(output protocol.Output, events []protocol.Event, failure error, secrets ...string) error {
	text := ""
	if output.Kind != "" {
		body, err := json.Marshal(output)
		if err != nil {
			return err
		}
		text += string(body)
	}
	for _, event := range events {
		body, err := json.Marshal(event)
		if err != nil {
			return err
		}
		text += string(body)
	}
	if failure != nil {
		text += failure.Error()
	}
	for _, secret := range secrets {
		if strings.Contains(text, secret) {
			return fmt.Errorf("a database secret reached a public value")
		}
	}
	return nil
}

func databaseFixturePassword(ctx context.Context, db *sql.Conn, id protocol.DatabaseID) (string, error) {
	var password string
	err := db.QueryRowContext(ctx, "SELECT password FROM database_profiles WHERE id=?1", id.String()).Scan(&password)
	return password, err
}

func testDatabaseProfiles(t *testing.T) {
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	first, second, tunnelSecret, sshSecret := "first-database-secret", "second-database-secret", "tunnel-database-secret", "ssh-login-secret"
	secrets := []string{first, second, tunnelSecret, sshSecret}
	write := func(t *testing.T, operation func(db *sql.Conn) error) {
		t.Helper()
		if err := storage.Node.Write(ctx, operation); err != nil {
			t.Fatal(err)
		}
	}
	var tunnel protocol.SshProfile
	write(t, func(db *sql.Conn) error {
		var err error
		tunnel, err = databaseFixtureSsh(ctx, db, protocol.SshProfile{ID: protocol.NewSshID(), Name: "Bastion", Host: "bastion.example", Port: 22, Username: "deploy", Authentication: protocol.SshAuthenticationPassword}, 0, &sshSecret)
		return err
	})

	t.Run("validation", func(t *testing.T) {
		long := strings.Repeat("x", 257)
		network := func(edit func(*protocol.DatabaseConnectionNetworkPayload)) protocol.DatabaseConnection {
			connection := databaseFixtureNetwork("db.example")
			edit(connection.Data.(*protocol.DatabaseConnectionNetworkPayload))
			return connection
		}
		rows := []struct {
			name       string
			profile    string
			connection protocol.DatabaseConnection
			password   *string
			expected   uint64
			code       protocol.ErrorCode
		}{
			{"blank name", "  ", databaseFixtureNetwork("db.example"), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"long name", long, databaseFixtureNetwork("db.example"), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"relative SQLite path", "Local", databaseFixtureSqlite("data/app.db"), nil, 0, protocol.ErrorCodeInvalidRequest},
			{"SQLite password", "Local", databaseFixtureSqlite(filepath.Join(t.TempDir(), "app.db")), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"empty host", "Remote", databaseFixtureNetwork(""), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"host whitespace", "Remote", databaseFixtureNetwork("db example"), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"host path", "Remote", databaseFixtureNetwork("db.example/app"), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"zero port", "Remote", network(func(target *protocol.DatabaseConnectionNetworkPayload) { target.Port = 0 }), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"SQLite network engine", "Remote", network(func(target *protocol.DatabaseConnectionNetworkPayload) { target.Engine = protocol.DatabaseEngineSqlite }), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"empty username", "Remote", network(func(target *protocol.DatabaseConnectionNetworkPayload) { target.Username = "" }), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"control database", "Remote", network(func(target *protocol.DatabaseConnectionNetworkPayload) { target.Database = "app\n" }), &first, 0, protocol.ErrorCodeInvalidRequest},
			{"missing tunnel", "Tunnel", databaseFixtureTunnel(protocol.NewSshID()), &first, 0, protocol.ErrorCodeNotFound},
			{"unknown revision", "Remote", databaseFixtureNetwork("db.example"), &first, 3, protocol.ErrorCodeRevisionConflict},
		}
		for _, row := range rows {
			t.Run(row.name, func(t *testing.T) {
				write(t, func(db *sql.Conn) error {
					output, events, err := databaseFixtureSave(ctx, db, protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: row.profile, Connection: row.connection}, row.expected, row.password)
					if !providerFixtureFault(err, row.code) {
						return fmt.Errorf("save returned %v, expected %s", err, row.code)
					}
					if err := databaseFixturePublic(output, events, err, secrets...); err != nil {
						return err
					}
					profiles, err := ListDatabases(ctx, db)
					if err == nil && len(profiles) != 0 {
						err = fmt.Errorf("a rejected profile was stored")
					}
					return err
				})
			})
		}
	})

	profile := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: "  Primary  ", Connection: databaseFixtureNetwork("db.example")}
	t.Run("password lifecycle", func(t *testing.T) {
		moved := databaseFixtureNetwork("replica.example")
		empty := ""
		rows := []struct {
			name       string
			connection protocol.DatabaseConnection
			password   *string
			expected   uint64
			code       protocol.ErrorCode
			stored     string
		}{
			{"create stores the password", profile.Connection, &first, 0, "", first},
			{"stale revision conflicts", profile.Connection, &second, 0, protocol.ErrorCodeRevisionConflict, first},
			{"omitted password is kept", profile.Connection, nil, 1, "", first},
			{"changed target requires a password", moved, nil, 2, protocol.ErrorCodeNotConfigured, first},
			{"changed target accepts a replacement", moved, &second, 2, "", second},
			{"replacement binds the new target", moved, nil, 3, "", second},
			{"former target is no longer bound", profile.Connection, nil, 4, protocol.ErrorCodeNotConfigured, second},
			{"empty password clears", moved, &empty, 4, "", ""},
		}
		for _, row := range rows {
			t.Run(row.name, func(t *testing.T) {
				write(t, func(db *sql.Conn) error {
					draft := profile
					draft.Connection = row.connection
					output, events, err := databaseFixtureSave(ctx, db, draft, row.expected, row.password)
					if row.code != "" {
						if !providerFixtureFault(err, row.code) {
							return fmt.Errorf("save returned %v, expected %s", err, row.code)
						}
					} else if err != nil {
						return err
					} else {
						saved := output.Data.(protocol.DatabaseProfile)
						if output.Kind != "database_profile" || saved.Revision != row.expected+1 || saved.Name != "Primary" ||
							len(events) != 1 || events[0].Kind != "database_changed" || events[0].Data.(protocol.DatabaseProfile).Revision != saved.Revision {
							return fmt.Errorf("save output or event changed: %+v", output)
						}
					}
					if err := databaseFixturePublic(output, events, err, secrets...); err != nil {
						return err
					}
					stored, err := databaseFixturePassword(ctx, db, profile.ID)
					if err == nil && stored != row.stored {
						err = fmt.Errorf("stored password does not match the expected lifecycle state")
					}
					return err
				})
			})
		}
	})

	tunnelled := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: "Tunnelled", Connection: databaseFixtureTunnel(tunnel.ID)}
	t.Run("tunnel destination binding", func(t *testing.T) {
		write(t, func(db *sql.Conn) error {
			if _, _, err := databaseFixtureSave(ctx, db, tunnelled, 0, &tunnelSecret); err != nil {
				return err
			}
			if _, _, err := databaseFixtureSave(ctx, db, tunnelled, 1, nil); err != nil {
				return fmt.Errorf("unchanged tunnel did not keep its password: %w", err)
			}
			check := protocol.Command{Kind: "check_database", Data: &protocol.CommandCheckDatabasePayload{Profile: tunnelled.ID, ExpectedRevision: 2}}
			operation, err := PrepareDatabase(ctx, db, storage.Profile, check)
			if err != nil {
				return err
			}
			if operation.Password.Expose() != tunnelSecret || operation.Ssh == nil || operation.Ssh.Profile.ID != tunnel.ID || operation.Ssh.Credential.Kind != "password" ||
				operation.Storage != storage.Profile || operation.Query != nil || operation.Browse != nil {
				return fmt.Errorf("prepared tunnel operation is incomplete")
			}
			renamed := tunnel
			renamed.Username = "operator"
			if tunnel, err = databaseFixtureSsh(ctx, db, renamed, tunnel.Revision, nil); err != nil {
				return err
			}
			_, err = PrepareDatabase(ctx, db, storage.Profile, check)
			if !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
				return fmt.Errorf("a changed SSH destination reused the database password: %v", err)
			}
			if err := databaseFixturePublic(protocol.Output{}, nil, err, secrets...); err != nil {
				return err
			}
			output, events, err := databaseFixtureSave(ctx, db, tunnelled, 2, nil)
			if !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
				return fmt.Errorf("a changed SSH destination kept the database password: %v", err)
			}
			if err := databaseFixturePublic(output, events, err, secrets...); err != nil {
				return err
			}
			if _, _, err := databaseFixtureSave(ctx, db, tunnelled, 2, &tunnelSecret); err != nil {
				return err
			}
			check.Data.(*protocol.CommandCheckDatabasePayload).ExpectedRevision = 3
			operation, err = PrepareDatabase(ctx, db, storage.Profile, check)
			if err == nil && (operation.Ssh == nil || operation.Ssh.Profile.Username != "operator") {
				err = fmt.Errorf("prepared tunnel does not use the current SSH profile")
			}
			return err
		})
	})

	t.Run("prepared operations", func(t *testing.T) {
		selected, invalidSelection := "analytics", "bad\nname"
		query := func(edit func(*protocol.CommandQueryDatabasePayload)) protocol.Command {
			input := &protocol.CommandQueryDatabasePayload{Profile: tunnelled.ID, ExpectedRevision: 3, Sql: "SELECT 1", RowLimit: 100, TimeoutMs: 1000}
			edit(input)
			return protocol.Command{Kind: "query_database", Data: input}
		}
		rows := []struct {
			name    string
			command protocol.Command
			code    protocol.ErrorCode
		}{
			{"blank SQL", query(func(input *protocol.CommandQueryDatabasePayload) { input.Sql = "  " }), protocol.ErrorCodeInvalidRequest},
			{"oversized SQL", query(func(input *protocol.CommandQueryDatabasePayload) { input.Sql = strings.Repeat("x", 65537) }), protocol.ErrorCodeInvalidRequest},
			{"zero rows", query(func(input *protocol.CommandQueryDatabasePayload) { input.RowLimit = 0 }), protocol.ErrorCodeInvalidRequest},
			{"too many rows", query(func(input *protocol.CommandQueryDatabasePayload) { input.RowLimit = 10_001 }), protocol.ErrorCodeInvalidRequest},
			{"zero timeout", query(func(input *protocol.CommandQueryDatabasePayload) { input.TimeoutMs = 0 }), protocol.ErrorCodeInvalidRequest},
			{"long timeout", query(func(input *protocol.CommandQueryDatabasePayload) { input.TimeoutMs = 3_600_001 }), protocol.ErrorCodeInvalidRequest},
			{"invalid selection", query(func(input *protocol.CommandQueryDatabasePayload) { input.Database = &invalidSelection }), protocol.ErrorCodeInvalidRequest},
			{"stale revision", query(func(input *protocol.CommandQueryDatabasePayload) { input.ExpectedRevision = 2 }), protocol.ErrorCodeRevisionConflict},
			{"missing profile", protocol.Command{Kind: "check_database", Data: &protocol.CommandCheckDatabasePayload{Profile: protocol.NewDatabaseID(), ExpectedRevision: 1}}, protocol.ErrorCodeNotFound},
			{"configuration command", protocol.Command{Kind: "list_databases"}, protocol.ErrorCodeInvalidRequest},
		}
		for _, row := range rows {
			t.Run(row.name, func(t *testing.T) {
				write(t, func(db *sql.Conn) error {
					_, err := PrepareDatabase(ctx, db, storage.Profile, row.command)
					if !providerFixtureFault(err, row.code) {
						return fmt.Errorf("prepare returned %v, expected %s", err, row.code)
					}
					return databaseFixturePublic(protocol.Output{}, nil, err, secrets...)
				})
			})
		}
		t.Run("selected database and read-only query", func(t *testing.T) {
			write(t, func(db *sql.Conn) error {
				operation, err := PrepareDatabase(ctx, db, storage.Profile, query(func(input *protocol.CommandQueryDatabasePayload) {
					input.Database, input.ReadOnly = &selected, true
				}))
				if err != nil {
					return err
				}
				target, valid := dispatchValue[protocol.DatabaseConnectionSshPayload](operation.Profile.Connection.Data)
				if !valid || target.Database != selected || !operation.Profile.ReadOnly || operation.Query == nil || operation.Query.RowLimit != 100 {
					return fmt.Errorf("prepared query lost its selection or limits")
				}
				saved, err := ReadDatabase(ctx, db, tunnelled.ID)
				if err != nil {
					return err
				}
				target, valid = dispatchValue[protocol.DatabaseConnectionSshPayload](saved.Connection.Data)
				if !valid || target.Database != "app" || saved.ReadOnly {
					return fmt.Errorf("a prepared operation changed the saved profile")
				}
				browse, err := PrepareDatabase(ctx, db, storage.Profile, protocol.Command{Kind: "browse_database", Data: &protocol.CommandBrowseDatabasePayload{Profile: tunnelled.ID, ExpectedRevision: 3}})
				if err == nil && (browse.Browse == nil || browse.Browse.Database != nil || browse.Query != nil) {
					err = fmt.Errorf("prepared browse lost its catalog request")
				}
				return err
			})
		})
		t.Run("draft test reuses only the saved target", func(t *testing.T) {
			write(t, func(db *sql.Conn) error {
				draft := tunnelled
				draft.Revision = 3
				operation, err := PrepareDatabase(ctx, db, storage.Profile, protocol.Command{Kind: "test_database", Data: &protocol.CommandTestDatabasePayload{Profile: draft, ExpectedRevision: 3}})
				if err != nil || operation.Password.Expose() != tunnelSecret {
					return fmt.Errorf("draft test did not reuse the saved password: %v", err)
				}
				draft.Connection = databaseFixtureNetwork("elsewhere.example")
				_, err = PrepareDatabase(ctx, db, storage.Profile, protocol.Command{Kind: "test_database", Data: &protocol.CommandTestDatabasePayload{Profile: draft, ExpectedRevision: 3}})
				if !providerFixtureFault(err, protocol.ErrorCodeNotConfigured) {
					return fmt.Errorf("draft test sent the saved password to another target: %v", err)
				}
				supplied := protocol.NewSecret(second)
				operation, err = PrepareDatabase(ctx, db, storage.Profile, protocol.Command{Kind: "test_database", Data: &protocol.CommandTestDatabasePayload{Profile: draft, ExpectedRevision: 3, Password: &supplied}})
				if err == nil && (operation.Password.Expose() != second || operation.Ssh != nil) {
					err = fmt.Errorf("draft test ignored its supplied password")
				}
				return err
			})
		})
	})

	t.Run("list and remove", func(t *testing.T) {
		write(t, func(db *sql.Conn) error {
			output, events, err := ExecuteDatabaseCommand(ctx, db, protocol.Command{Kind: "list_databases"})
			if err != nil {
				return err
			}
			if output.Kind != "database_profiles" || len(output.Data.([]protocol.DatabaseProfile)) != 2 || len(events) != 0 {
				return fmt.Errorf("list output changed: %+v", output)
			}
			if err := databaseFixturePublic(output, events, nil, secrets...); err != nil {
				return err
			}
			remove := &protocol.CommandRemoveDatabasePayload{Profile: tunnelled.ID, ExpectedRevision: 2}
			if _, _, err := ExecuteDatabaseCommand(ctx, db, protocol.Command{Kind: "remove_database", Data: remove}); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
				return fmt.Errorf("stale removal returned %v", err)
			}
			remove.ExpectedRevision = 3
			output, events, err = ExecuteDatabaseCommand(ctx, db, protocol.Command{Kind: "remove_database", Data: remove})
			if err != nil {
				return err
			}
			remaining := output.Data.([]protocol.DatabaseProfile)
			if len(remaining) != 1 || remaining[0].ID != profile.ID || len(events) != 1 || events[0].Kind != "database_removed" ||
				events[0].Data.(protocol.EventDatabaseRemovedPayload).ID != tunnelled.ID {
				return fmt.Errorf("removal output or event changed: %+v", output)
			}
			if err := databaseFixturePublic(output, events, nil, secrets...); err != nil {
				return err
			}
			if _, _, err := ExecuteDatabaseCommand(ctx, db, protocol.Command{Kind: "remove_database", Data: remove}); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
				return fmt.Errorf("repeated removal returned %v", err)
			}
			return nil
		})
	})
}

func testConnectionWorkspaces(t *testing.T) {
	storage, node, used, turn, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	write := func(t *testing.T, operation func(db *sql.Conn) error) {
		t.Helper()
		if err := storage.Node.Write(ctx, operation); err != nil {
			t.Fatal(err)
		}
	}
	project := protocol.NewProjectID()
	database := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: "Local", Connection: databaseFixtureSqlite(filepath.Join(t.TempDir(), "app.db")), Sharing: &protocol.ConnectionSharing{Kind: "global"}}
	private := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: "Private", Connection: databaseFixtureSqlite(filepath.Join(t.TempDir(), "private.db"))}
	var remote protocol.SshProfile
	write(t, func(db *sql.Conn) error {
		for _, profile := range []protocol.DatabaseProfile{database, private} {
			if _, _, err := databaseFixtureSave(ctx, db, profile, 0, nil); err != nil {
				return err
			}
		}
		secret := "ssh-login-secret"
		var err error
		remote, err = databaseFixtureSsh(ctx, db, protocol.SshProfile{ID: protocol.NewSshID(), Name: "Remote", Host: "remote.example", Port: 22, Username: "deploy", Authentication: protocol.SshAuthenticationPassword}, 0, &secret)
		return err
	})
	databaseTarget, privateTarget, sshTarget := databaseResource(database.ID), databaseResource(private.ID), sshResource(remote.ID)

	t.Run("workspace reuse", func(t *testing.T) {
		rows := []struct {
			name     string
			resource protocol.ConnectionResource
		}{
			{"database", databaseTarget},
			{"decoded database", protocol.ConnectionResource{Kind: "database", Data: &database.ID}},
			{"SSH", sshTarget},
		}
		for _, row := range rows {
			t.Run(row.name, func(t *testing.T) {
				write(t, func(db *sql.Conn) error {
					first, err := connectionWorkspace(ctx, db, storage.Profile, row.resource)
					if err != nil {
						return err
					}
					second, err := connectionWorkspace(ctx, db, storage.Profile, row.resource)
					if err != nil {
						return err
					}
					root, err := core.ConnectionWorkspace(storage.Profile, row.resource)
					if err != nil {
						return err
					}
					var count int
					var id string
					if err := db.QueryRowContext(ctx, "SELECT COUNT(*),MIN(id) FROM worktrees WHERE path=?1 AND project IS NULL AND main=0", root).Scan(&count, &id); err != nil {
						return err
					}
					if first != second || count != 1 || id != first.String() || !core.ManagedWorkspace(root, storage.Profile) {
						return fmt.Errorf("connection workspace was not reused at its managed path")
					}
					return nil
				})
			})
		}
		write(t, func(db *sql.Conn) error {
			left, err := connectionWorkspace(ctx, db, storage.Profile, databaseTarget)
			if err != nil {
				return err
			}
			right, err := connectionWorkspace(ctx, db, storage.Profile, sshTarget)
			if err == nil && left == right {
				err = fmt.Errorf("distinct connections share one workspace")
			}
			return err
		})
	})

	t.Run("workspace refusal", func(t *testing.T) {
		rows := []struct {
			name     string
			profile  string
			resource protocol.ConnectionResource
			code     protocol.ErrorCode
		}{
			{"missing database", storage.Profile, databaseResource(protocol.NewDatabaseID()), protocol.ErrorCodeNotFound},
			{"missing SSH", storage.Profile, sshResource(protocol.NewSshID()), protocol.ErrorCodeNotFound},
			{"mismatched identifier", storage.Profile, protocol.ConnectionResource{Kind: "ssh", Data: database.ID}, protocol.ErrorCodeInvalidRequest},
			{"missing storage", "", databaseTarget, protocol.ErrorCodeNotConfigured},
		}
		for _, row := range rows {
			t.Run(row.name, func(t *testing.T) {
				write(t, func(db *sql.Conn) error {
					if _, err := connectionWorkspace(ctx, db, row.profile, row.resource); !providerFixtureFault(err, row.code) {
						return fmt.Errorf("workspace returned %v, expected %s", err, row.code)
					}
					return nil
				})
			})
		}
	})

	t.Run("sharing decision", func(t *testing.T) {
		other := protocol.NewProjectID()
		global := &protocol.ConnectionSharing{Kind: "global"}
		listed := &protocol.ConnectionSharing{Kind: "projects", Data: &[]protocol.ProjectID{project}}
		rows := []struct {
			name    string
			bound   *protocol.ConnectionResource
			project *protocol.ProjectID
			sharing *protocol.ConnectionSharing
			allowed bool
		}{
			{"bound to the target", &databaseTarget, nil, nil, true},
			{"bound to another resource", &sshTarget, &project, global, false},
			{"project with global sharing", nil, &project, global, true},
			{"project in the list", nil, &project, listed, true},
			{"project outside the list", nil, &other, listed, false},
			{"project without sharing", nil, &project, nil, false},
			{"unassigned conversation", nil, nil, global, false},
		}
		for _, row := range rows {
			t.Run(row.name, func(t *testing.T) {
				if ConnectionPermits(row.bound, row.project, databaseTarget, row.sharing) != row.allowed {
					t.Fatal("sharing decision changed")
				}
			})
		}
	})

	t.Run("session binding", func(t *testing.T) {
		write(t, func(db *sql.Conn) error {
			output, _, err := createSession(ctx, db, node, storage.Profile, protocol.CommandCreateSessionPayload{Config: &used.Config})
			if err != nil {
				return err
			}
			fresh := output.Data.(protocol.Session)
			bind := protocol.CommandBindConnectionSessionPayload{Session: fresh.ID, ExpectedRevision: fresh.Revision + 1, Resource: databaseTarget}
			if _, _, err := bindConnectionSession(ctx, db, storage.Profile, bind); !providerFixtureFault(err, protocol.ErrorCodeRevisionConflict) {
				return fmt.Errorf("stale binding returned %v", err)
			}
			bind.ExpectedRevision = fresh.Revision
			bind.Resource = databaseResource(protocol.NewDatabaseID())
			if _, _, err := bindConnectionSession(ctx, db, storage.Profile, bind); !providerFixtureFault(err, protocol.ErrorCodeNotFound) {
				return fmt.Errorf("binding a missing connection returned %v", err)
			}
			bind.Resource = databaseTarget
			output, events, err := bindConnectionSession(ctx, db, storage.Profile, bind)
			if err != nil {
				return err
			}
			bound := output.Data.(protocol.Session)
			workspace, err := connectionWorkspace(ctx, db, storage.Profile, databaseTarget)
			if err != nil {
				return err
			}
			if bound.Revision != fresh.Revision+1 || bound.Project != nil || bound.Worktree != workspace || bound.Config.Resource == nil ||
				!sameConnection(*bound.Config.Resource, databaseTarget) || len(events) != 1 || events[0].Kind != "session_changed" {
				return fmt.Errorf("bound session changed: %+v", bound)
			}
			stored, err := ReadSession(ctx, db, fresh.ID)
			if err != nil {
				return err
			}
			if stored.Worktree != workspace || stored.Config.Resource == nil || !sameConnection(*stored.Config.Resource, databaseTarget) {
				return fmt.Errorf("binding was not stored")
			}
			bind.ExpectedRevision = bound.Revision
			bind.Resource = sshTarget
			if _, _, err := bindConnectionSession(ctx, db, storage.Profile, bind); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
				return fmt.Errorf("rebinding returned %v", err)
			}
			submitted, err := ReadSession(ctx, db, used.ID)
			if err != nil {
				return err
			}
			if _, _, err := bindConnectionSession(ctx, db, storage.Profile, protocol.CommandBindConnectionSessionPayload{Session: used.ID, ExpectedRevision: submitted.Revision, Resource: databaseTarget}); !providerFixtureFault(err, protocol.ErrorCodeConflict) {
				return fmt.Errorf("binding a used conversation returned %v", err)
			}
			catalog, err := ReadConnectionCatalog(ctx, db, fresh.ID)
			if err != nil {
				return err
			}
			if catalog.Bound == nil || len(catalog.Databases) != 1 || catalog.Databases[0].ID != database.ID || len(catalog.Ssh) != 0 || len(catalog.Resources()) != 1 {
				return fmt.Errorf("bound catalog exposes other connections")
			}
			catalog, err = ReadConnectionCatalog(ctx, db, used.ID)
			if err == nil && len(catalog.Resources()) != 0 {
				err = fmt.Errorf("unassigned conversation sees shared connections")
			}
			return err
		})
	})

	t.Run("captured turn access", func(t *testing.T) {
		owner := protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64)}
		captured := []protocol.ConnectionResource{databaseTarget, privateTarget, sshTarget}
		write(t, func(db *sql.Conn) error {
			created, err := ConnectionCreated(ctx, db, turn, owner, databaseTarget)
			if err != nil || created {
				return fmt.Errorf("a connection without an approved save counts as created: %v", err)
			}
			// An approved save is kept by digest in the ledger; the approval
			// keeps the command without its password and still proves creation.
			password, operation := protocol.NewSecret("created-connection-password"), protocol.ToolOperationSaveDatabaseConnection
			added := protocol.DatabaseProfile{ID: protocol.NewDatabaseID(), Name: "Created", Connection: databaseFixtureSqlite(filepath.Join(t.TempDir(), "created.db"))}
			save := protocol.NewRequest(node, protocol.Command{Kind: "save_database", Data: protocol.CommandSaveDatabasePayload{Profile: added, Password: &password}})
			scope := protocol.DefaultPluginContext()
			scope.Package, scope.Turn, scope.Session = owner, &turn, &used.ID
			save.Plugin = &scope
			approval := protocol.DefaultConversationApproval()
			approval.ID, approval.Session, approval.Turn = protocol.NewApprovalID(), used.ID, turn
			approval.State, approval.Source = protocol.ConversationApprovalStateApproved, protocol.ConversationApprovalSourceUser
			approved, err := encode(approval)
			if err != nil {
				return err
			}
			bound, err := encode(ApprovalBinding{Owner: owner, Tool: "save_connection", Operation: &operation})
			if err != nil {
				return err
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO agent_approvals(id,turn,call,body,binding,request,claimed) VALUES(?1,?2,'save-call',?3,?4,?5,1)", approval.ID.String(), turn.String(), approved, bound, save.ID.String()); err != nil {
				return err
			}
			body, err := encode(save)
			if err != nil {
				return err
			}
			if _, err = admission.Admit(ctx, db, node, save, body); err != nil {
				return err
			}
			if creation, err := authorizeConnectionManagement(ctx, db, save); err != nil || !creation {
				return fmt.Errorf("approved save was not authorized as a creation: %v", err)
			}
			added.Revision = 1
			if err = admission.Complete(ctx, db, node, save.ID, link.Response{Output: protocol.Output{Kind: "database_profile", Data: added}}); err != nil {
				return err
			}
			var ledger, kept []byte
			if err = db.QueryRowContext(ctx, "SELECT r.body,a.prepared FROM requests r JOIN agent_approvals a ON a.request=r.id WHERE r.id=?1", save.ID.String()).Scan(&ledger, &kept); err != nil {
				return err
			}
			if !strings.HasPrefix(string(ledger), "credential-v1:") || strings.Contains(string(kept), password.Expose()) || !strings.Contains(string(kept), added.ID.String()) {
				return fmt.Errorf("approved save was stored as %s and %s", ledger, kept)
			}
			if created, err = ConnectionCreated(ctx, db, turn, owner, databaseResource(added.ID)); err != nil || !created {
				return fmt.Errorf("an approved completed save does not count as created: %v", err)
			}
			if created, err = ConnectionCreated(ctx, db, turn, owner, databaseTarget); err != nil || created {
				return fmt.Errorf("another connection counts as created: %v", err)
			}
			statuses, err := ConnectionStatuses(ctx, db, turn, owner, captured, captured)
			if err != nil || len(statuses) != 0 {
				return fmt.Errorf("an unassigned turn reached connections: %v, %d", err, len(statuses))
			}
			timeout := uint64(1000)
			query := ConnectionAction{Kind: ConnectionQuery, Sql: "SELECT 1", RowLimit: 10, TimeoutMs: &timeout}
			if _, err := ConnectionCommand(ctx, db, turn, owner, &privateTarget, query, captured); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
				return fmt.Errorf("an unshared connection was reachable: %v", err)
			}
			if _, err := ConnectionCommand(ctx, db, turn, owner, nil, query, captured); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
				return fmt.Errorf("a missing target was accepted: %v", err)
			}
			listed, err := ConnectionCommand(ctx, db, turn, owner, nil, ConnectionAction{Kind: ConnectionListDatabases}, nil)
			if err != nil || listed.Kind != "list_databases" {
				return fmt.Errorf("catalog listing changed: %v", err)
			}
			if bound, err := ConnectionBound(ctx, db, turn); err != nil || bound != nil {
				return fmt.Errorf("unbound turn reports a binding: %v", err)
			}
			run, err := ConnectionOwner(ctx, db, turn)
			if err != nil {
				return err
			}
			var root string
			if err := db.QueryRowContext(ctx, "SELECT path FROM worktrees WHERE id=?1", run.Worktree.String()).Scan(&root); err != nil {
				return err
			}
			name, relative := "Scratch", "./data/scratch.db"
			engine := protocol.DatabaseEngineSqlite
			saved, err := ConnectionCommand(ctx, db, turn, owner, nil, ConnectionAction{Kind: ConnectionSaveDatabase, DatabaseInput: ConnectionDatabaseInput{Name: &name, Engine: &engine, Path: &relative}}, nil)
			if err != nil {
				return err
			}
			input := saved.Data.(*protocol.CommandSaveDatabasePayload)
			target, valid := dispatchValue[protocol.DatabaseConnectionSqlitePayload](input.Profile.Connection.Data)
			if saved.Kind != "save_database" || input.ExpectedRevision != 0 || input.Password != nil || input.Profile.Sharing != nil || !input.Profile.ReadOnly ||
				!valid || target.Path != filepath.Join(root, "data", "scratch.db") {
				return fmt.Errorf("created connection command changed: %+v", input.Profile)
			}
			for _, escape := range []string{"../scratch.db", "data/../scratch.db", "././scratch.db", "", "a//b", ".sailry/worktrees/other/scratch.db"} {
				if _, err := connectionSqlitePath(ctx, db, turn, escape); err == nil {
					return fmt.Errorf("escaping SQLite path %q was accepted", escape)
				}
			}
			revision := uint64(1)
			if _, err := ConnectionCommand(ctx, db, turn, owner, nil, ConnectionAction{Kind: ConnectionSaveDatabase, DatabaseInput: ConnectionDatabaseInput{Connection: &private.ID, ExpectedRevision: &revision, Name: &name}}, captured); !providerFixtureFault(err, protocol.ErrorCodePermissionDenied) {
				return fmt.Errorf("an unshared connection was editable: %v", err)
			}
			if _, err := ConnectionCommand(ctx, db, turn, owner, nil, ConnectionAction{Kind: ConnectionSaveDatabase, DatabaseInput: ConnectionDatabaseInput{Connection: &private.ID, Name: &name}}, captured); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
				return fmt.Errorf("an update without its revision was accepted: %v", err)
			}
			authentication := protocol.SshAuthenticationPassword
			host, port, username := "new.example", uint16(22), "deploy"
			if _, err := ConnectionCommand(ctx, db, turn, owner, nil, ConnectionAction{Kind: ConnectionSaveSsh, SshInput: ConnectionSshInput{Name: &name, Host: &host, Port: &port, Username: &username, Authentication: &authentication}}, nil); !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
				return fmt.Errorf("a new SSH connection without a credential was accepted: %v", err)
			}
			return nil
		})
	})
}
