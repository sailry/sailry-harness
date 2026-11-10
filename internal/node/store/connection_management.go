package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"io/fs"
	"path/filepath"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConnectionDatabaseInput is a conversation's edit of a database connection.
// It carries public configuration only; a stored password is preserved.
type ConnectionDatabaseInput struct {
	Connection       *protocol.DatabaseID     `json:"connection"`
	ExpectedRevision *uint64                  `json:"expected_revision"`
	Name             *string                  `json:"name"`
	Engine           *protocol.DatabaseEngine `json:"engine"`
	Path             *string                  `json:"path"`
	Host             *string                  `json:"host"`
	Port             *uint16                  `json:"port"`
	Username         *string                  `json:"username"`
	Database         *string                  `json:"database"`
	ReadOnly         *bool                    `json:"read_only"`
}

func (value *ConnectionDatabaseInput) UnmarshalJSON(data []byte) error {
	type plain ConnectionDatabaseInput
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, nil, true); err != nil {
		return err
	}
	*value = ConnectionDatabaseInput(decoded)
	return nil
}

// ConnectionSshInput is a conversation's edit of an SSH connection.
type ConnectionSshInput struct {
	Connection       *protocol.SshID             `json:"connection"`
	ExpectedRevision *uint64                     `json:"expected_revision"`
	Name             *string                     `json:"name"`
	Host             *string                     `json:"host"`
	Port             *uint16                     `json:"port"`
	Username         *string                     `json:"username"`
	Authentication   *protocol.SshAuthentication `json:"authentication"`
	Credential       *protocol.SshCredential     `json:"credential"`
	KeyPath          *string                     `json:"key_path"`
}

func (value *ConnectionSshInput) UnmarshalJSON(data []byte) error {
	type plain ConnectionSshInput
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, nil, true); err != nil {
		return err
	}
	*value = ConnectionSshInput(decoded)
	return nil
}

func connectionRequired[Value any](value *Value, name string) (Value, error) {
	if value == nil {
		var empty Value
		return empty, invalid("connection requires " + name)
	}
	return *value, nil
}

func connectionFirst[Value any](supplied, existing *Value) *Value {
	if supplied != nil {
		return supplied
	}
	return existing
}

func connectionRevision(update bool, revision *uint64) (uint64, error) {
	switch {
	case !update && revision == nil:
		return 0, nil
	case update && revision != nil && *revision > 0:
		return *revision, nil
	}
	return 0, invalid("connection update requires its exact revision")
}

func connectionSaveDatabase(ctx context.Context, db *sql.Conn, turn protocol.TurnID, owner protocol.PluginReference, captured []protocol.ConnectionResource, input ConnectionDatabaseInput) (protocol.Command, error) {
	expected, err := connectionRevision(input.Connection != nil, input.ExpectedRevision)
	if err != nil {
		return protocol.Command{}, err
	}
	var previous *protocol.DatabaseProfile
	if input.Connection != nil {
		if err := checkConnection(ctx, db, turn, owner, databaseResource(*input.Connection), captured); err != nil {
			return protocol.Command{}, err
		}
		if previous, err = ReadDatabase(ctx, db, *input.Connection); err != nil {
			return protocol.Command{}, err
		}
		if previous == nil {
			return protocol.Command{}, fault(protocol.ErrorCodeNotFound, "database connection does not exist")
		}
	}
	var profile protocol.DatabaseProfile
	if previous != nil {
		profile = *previous
	} else {
		if err := connectionCreation(ctx, db, turn); err != nil {
			return protocol.Command{}, err
		}
		sharing, err := connectionTurnSharing(ctx, db, turn)
		if err != nil {
			return protocol.Command{}, err
		}
		profile = protocol.DatabaseProfile{Sharing: sharing, ID: protocol.NewDatabaseID(), Connection: protocol.DatabaseConnection{Kind: "sqlite", Data: &protocol.DatabaseConnectionSqlitePayload{}}, ReadOnly: true}
	}
	if input.Name != nil {
		profile.Name = *input.Name
	}
	if input.ReadOnly != nil {
		profile.ReadOnly = *input.ReadOnly
	}
	edited := input.Engine != nil || input.Path != nil || input.Host != nil || input.Port != nil || input.Username != nil || input.Database != nil
	if previous == nil || edited {
		var saved *protocol.DatabaseEngine
		if previous != nil {
			saved = connectionEngine(previous.Connection)
		}
		engine, err := connectionRequired(connectionFirst(input.Engine, saved), "engine")
		if err != nil {
			return protocol.Command{}, err
		}
		if engine == protocol.DatabaseEngineSqlite {
			if input.Host != nil || input.Port != nil || input.Username != nil || input.Database != nil {
				return protocol.Command{}, invalid("SQLite connection accepts only a path")
			}
			var path string
			if input.Path != nil {
				if path, err = connectionSqlitePath(ctx, db, turn, *input.Path); err != nil {
					return protocol.Command{}, err
				}
			} else {
				var existing *string
				if target, valid := dispatchValue[protocol.DatabaseConnectionSqlitePayload](profile.Connection.Data); valid && previous != nil && profile.Connection.Kind == "sqlite" {
					existing = &target.Path
				}
				if path, err = connectionRequired(existing, "path"); err != nil {
					return protocol.Command{}, err
				}
			}
			profile.Connection = protocol.DatabaseConnection{Kind: "sqlite", Data: &protocol.DatabaseConnectionSqlitePayload{Path: path}}
		} else {
			if input.Path != nil {
				return protocol.Command{}, invalid("network database connection does not accept a path")
			}
			if input.Engine == nil && (profile.Connection.Kind == "socket" || profile.Connection.Kind == "ssh") {
				return protocol.Command{}, invalid("changing a socket or tunnel requires an explicit engine")
			}
			target := protocol.DatabaseConnectionNetworkPayload{Engine: engine, Tls: protocol.DatabaseTlsPrefer}
			var host, username, database *string
			var port *uint16
			if existing, valid := dispatchValue[protocol.DatabaseConnectionNetworkPayload](profile.Connection.Data); valid && profile.Connection.Kind == "network" {
				host, port, username, database, target.Tls = &existing.Host, &existing.Port, &existing.Username, &existing.Database, existing.Tls
			}
			if target.Host, err = connectionRequired(connectionFirst(input.Host, host), "host"); err != nil {
				return protocol.Command{}, err
			}
			if target.Port, err = connectionRequired(connectionFirst(input.Port, port), "port"); err != nil {
				return protocol.Command{}, err
			}
			if target.Username, err = connectionRequired(connectionFirst(input.Username, username), "username"); err != nil {
				return protocol.Command{}, err
			}
			if target.Database, err = connectionRequired(connectionFirst(input.Database, database), "database"); err != nil {
				return protocol.Command{}, err
			}
			profile.Connection = protocol.DatabaseConnection{Kind: "network", Data: &target}
		}
	}
	return protocol.Command{Kind: "save_database", Data: &protocol.CommandSaveDatabasePayload{Profile: profile, ExpectedRevision: expected}}, nil
}

func connectionEngine(connection protocol.DatabaseConnection) *protocol.DatabaseEngine {
	engine := protocol.DatabaseEngineSqlite
	switch connection.Kind {
	case "sqlite":
	case "network":
		target, valid := dispatchValue[protocol.DatabaseConnectionNetworkPayload](connection.Data)
		if !valid {
			return nil
		}
		engine = target.Engine
	case "socket":
		target, valid := dispatchValue[protocol.DatabaseConnectionSocketPayload](connection.Data)
		if !valid {
			return nil
		}
		engine = target.Engine
	case "ssh":
		target, valid := dispatchValue[protocol.DatabaseConnectionSshPayload](connection.Data)
		if !valid {
			return nil
		}
		engine = target.Engine
	default:
		return nil
	}
	return &engine
}

// connectionSqlitePath keeps an absolute path and resolves a normalized
// relative one inside the turn's registered worktree.
func connectionSqlitePath(ctx context.Context, db *sql.Conn, turn protocol.TurnID, path string) (string, error) {
	if filepath.IsAbs(path) {
		return path, nil
	}
	relative := strings.TrimPrefix(path, "./")
	if !fs.ValidPath(relative) || relative == "." || strings.ContainsAny(relative, "\\:") || strings.ContainsFunc(relative, unicode.IsControl) {
		return "", invalid("expected a normalized worktree-relative path")
	}
	parts := strings.Split(relative, "/")
	if len(parts) >= 2 && strings.EqualFold(parts[0], ".sailry") && strings.EqualFold(parts[1], "worktrees") {
		return "", fault(protocol.ErrorCodePermissionDenied, "managed worktrees require their own file scope")
	}
	run, err := activeAgentRun(ctx, db, turn)
	if err != nil {
		return "", err
	}
	var root string
	err = db.QueryRowContext(ctx, "SELECT path FROM worktrees WHERE id=?1", run.Worktree.String()).Scan(&root)
	if errors.Is(err, sql.ErrNoRows) {
		return "", fault(protocol.ErrorCodeNotFound, "worktree does not exist")
	}
	if err != nil {
		return "", err
	}
	if !filepath.IsAbs(root) {
		return "", invalid("worktree path must be absolute")
	}
	return filepath.Join(append([]string{root}, parts...)...), nil
}

func connectionSaveSsh(ctx context.Context, db *sql.Conn, turn protocol.TurnID, owner protocol.PluginReference, captured []protocol.ConnectionResource, input ConnectionSshInput) (protocol.Command, error) {
	expected, err := connectionRevision(input.Connection != nil, input.ExpectedRevision)
	if err != nil {
		return protocol.Command{}, err
	}
	var previous *protocol.SshProfile
	if input.Connection != nil {
		if err := checkConnection(ctx, db, turn, owner, sshResource(*input.Connection), captured); err != nil {
			return protocol.Command{}, err
		}
		if previous, err = readBody[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles WHERE id=?1", input.Connection.String()); err != nil {
			return protocol.Command{}, err
		}
		if previous == nil {
			return protocol.Command{}, fault(protocol.ErrorCodeNotFound, "SSH connection does not exist")
		}
	} else if err := connectionCreation(ctx, db, turn); err != nil {
		return protocol.Command{}, err
	}
	saved := protocol.SshProfile{}
	var name, host, username *string
	var port *uint16
	var savedAuthentication *protocol.SshAuthentication
	if previous != nil {
		saved = *previous
		name, host, username, port, savedAuthentication = &saved.Name, &saved.Host, &saved.Username, &saved.Port, &saved.Authentication
	}
	authentication, err := connectionRequired(connectionFirst(input.Authentication, savedAuthentication), "authentication")
	if err != nil {
		return protocol.Command{}, err
	}
	if input.Credential != nil && input.KeyPath != nil {
		return protocol.Command{}, invalid("SSH connection accepts a credential or a key path, not both")
	}
	keeps := previous != nil && previous.Authentication == authentication
	var credential *protocol.SshCredential
	switch {
	case input.Credential != nil:
		if input.Credential.Kind != string(authentication) {
			return protocol.Command{}, invalid("SSH credential authentication does not match")
		}
		if located, valid := dispatchValue[protocol.SshCredentialKeyPathPayload](input.Credential.Data); valid && input.Credential.Kind == "key_path" {
			if err := connectionKeyPath(located.Path.Expose()); err != nil {
				return protocol.Command{}, err
			}
		}
		credential = input.Credential
	case input.KeyPath != nil:
		if err := connectionKeyPath(*input.KeyPath); err != nil {
			return protocol.Command{}, err
		}
		if authentication != protocol.SshAuthenticationKeyPath {
			return protocol.Command{}, invalid("SSH key path requires an absolute path and key_path authentication")
		}
		located := protocol.SshCredentialKeyPathPayload{Path: protocol.NewSecret(*input.KeyPath)}
		if keeps {
			// A path edit must retain the existing optional passphrase inside the Node.
			var body string
			if err := db.QueryRowContext(ctx, "SELECT credential FROM ssh_profiles WHERE id=?1", previous.ID.String()).Scan(&body); err != nil {
				return protocol.Command{}, err
			}
			var existing protocol.SshCredential
			if err := json.Unmarshal([]byte(body), &existing); err != nil {
				return protocol.Command{}, err
			}
			stored, valid := dispatchValue[protocol.SshCredentialKeyPathPayload](existing.Data)
			if !valid || existing.Kind != "key_path" {
				return protocol.Command{}, invalid("SSH credential does not match its profile")
			}
			located.Passphrase = stored.Passphrase
		}
		credential = &protocol.SshCredential{Kind: "key_path", Data: &located}
	case keeps:
	case authentication == protocol.SshAuthenticationAgent:
		credential = &protocol.SshCredential{Kind: "agent"}
	default:
		return protocol.Command{}, invalid("SSH credential is required")
	}
	profile := protocol.SshProfile{ID: saved.ID, Sharing: saved.Sharing, Authentication: authentication, HostKey: saved.HostKey}
	if previous == nil {
		profile.ID = protocol.NewSshID()
		if profile.Sharing, err = connectionTurnSharing(ctx, db, turn); err != nil {
			return protocol.Command{}, err
		}
	}
	if profile.Name, err = connectionRequired(connectionFirst(input.Name, name), "name"); err != nil {
		return protocol.Command{}, err
	}
	if profile.Host, err = connectionRequired(connectionFirst(input.Host, host), "host"); err != nil {
		return protocol.Command{}, err
	}
	if profile.Port, err = connectionRequired(connectionFirst(input.Port, port), "port"); err != nil {
		return protocol.Command{}, err
	}
	if profile.Username, err = connectionRequired(connectionFirst(input.Username, username), "username"); err != nil {
		return protocol.Command{}, err
	}
	return protocol.Command{Kind: "save_ssh", Data: &protocol.CommandSaveSshPayload{Profile: profile, ExpectedRevision: expected, Credential: credential}}, nil
}

func connectionKeyPath(path string) error {
	if len(path) > 4096 || strings.ContainsRune(path, 0) || !filepath.IsAbs(path) {
		return invalid("SSH key path must be absolute")
	}
	return nil
}

// connectionCreation refuses a new connection from a conversation that is
// already bound to one.
func connectionCreation(ctx context.Context, db *sql.Conn, turn protocol.TurnID) error {
	if _, err := activeAgentRun(ctx, db, turn); err != nil {
		return err
	}
	capture, err := readTurnCapture(ctx, db, turn)
	if err != nil {
		return err
	}
	if capture.Config.Resource != nil {
		return fault(protocol.ErrorCodePermissionDenied, "bound conversations cannot create another connection")
	}
	return nil
}

// connectionTurnSharing shares a connection created in a project conversation
// with that project only.
func connectionTurnSharing(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (*protocol.ConnectionSharing, error) {
	run, err := activeAgentRun(ctx, db, turn)
	if err != nil {
		return nil, err
	}
	session, err := ReadSession(ctx, db, run.Session)
	if err != nil || session.Project == nil {
		return nil, err
	}
	return &protocol.ConnectionSharing{Kind: "projects", Data: []protocol.ProjectID{*session.Project}}, nil
}
