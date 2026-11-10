package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConnectionCatalog is what one conversation may see of the saved database
// and SSH profiles. Credentials stay behind the Node workers.
type ConnectionCatalog struct {
	Bound     *protocol.ConnectionResource
	Databases []protocol.DatabaseProfile
	Ssh       []protocol.SshProfile
}

func (catalog ConnectionCatalog) Resources() []protocol.ConnectionResource {
	resources := make([]protocol.ConnectionResource, 0, len(catalog.Databases)+len(catalog.Ssh))
	for _, profile := range catalog.Databases {
		resources = append(resources, databaseResource(profile.ID))
	}
	for _, profile := range catalog.Ssh {
		resources = append(resources, sshResource(profile.ID))
	}
	return resources
}

func ReadConnectionCatalog(ctx context.Context, db *sql.Conn, id protocol.SessionID) (ConnectionCatalog, error) {
	session, err := ReadSession(ctx, db, id)
	if err != nil {
		return ConnectionCatalog{}, err
	}
	catalog := ConnectionCatalog{Bound: session.Config.Resource, Databases: []protocol.DatabaseProfile{}, Ssh: []protocol.SshProfile{}}
	databases, err := ListDatabases(ctx, db)
	if err != nil {
		return catalog, err
	}
	for _, profile := range databases {
		if ConnectionPermits(session.Config.Resource, session.Project, databaseResource(profile.ID), profile.Sharing) {
			catalog.Databases = append(catalog.Databases, profile)
		}
	}
	profiles, err := ListSsh(ctx, db)
	if err != nil {
		return catalog, err
	}
	for _, profile := range profiles {
		if ConnectionPermits(session.Config.Resource, session.Project, sshResource(profile.ID), profile.Sharing) {
			catalog.Ssh = append(catalog.Ssh, profile)
		}
	}
	return catalog, nil
}

type ConnectionActionKind uint8

const (
	ConnectionListDatabases ConnectionActionKind = iota
	ConnectionListSsh
	ConnectionSaveDatabase
	ConnectionSaveSsh
	ConnectionBrowse
	ConnectionQuery
	ConnectionRun
	ConnectionTransfer
)

// ConnectionAction is one captured tool request. Only the fields of its kind
// are read.
type ConnectionAction struct {
	Kind ConnectionActionKind
	// SaveDatabase and SaveSsh.
	DatabaseInput ConnectionDatabaseInput
	SshInput      ConnectionSshInput
	// Browse and Query.
	Database *string
	ReadOnly bool
	Sql      string
	RowLimit uint32
	// Run and Transfer. Run defaults to 120 seconds; Query and Transfer
	// require a value.
	Command    string
	TimeoutMs  *uint64
	Path       string
	RemotePath string
	Direction  protocol.SshDirection
}

// ConnectionCommand translates a captured tool request into the shared command
// the Node workers execute, after checking the turn may reach its target.
func ConnectionCommand(ctx context.Context, db *sql.Conn, turn protocol.TurnID, owner protocol.PluginReference, target *protocol.ConnectionResource, action ConnectionAction, captured []protocol.ConnectionResource) (protocol.Command, error) {
	run, err := activeAgentRun(ctx, db, turn)
	if err != nil {
		return protocol.Command{}, err
	}
	switch action.Kind {
	case ConnectionListDatabases:
		return protocol.Command{Kind: "list_databases"}, nil
	case ConnectionListSsh:
		return protocol.Command{Kind: "list_ssh"}, nil
	case ConnectionSaveDatabase:
		return connectionSaveDatabase(ctx, db, turn, owner, captured, action.DatabaseInput)
	case ConnectionSaveSsh:
		return connectionSaveSsh(ctx, db, turn, owner, captured, action.SshInput)
	}
	if target == nil {
		return protocol.Command{}, invalid("connection target is required")
	}
	if err := checkConnection(ctx, db, turn, owner, *target, captured); err != nil {
		return protocol.Command{}, err
	}
	denied := fault(protocol.ErrorCodePermissionDenied, "connection is not available to this conversation")
	timeout := uint64(0)
	if action.TimeoutMs != nil {
		timeout = *action.TimeoutMs
	}
	if id, valid := dispatchValue[protocol.DatabaseID](target.Data); valid && target.Kind == "database" {
		profile, err := ReadDatabase(ctx, db, id)
		if err != nil {
			return protocol.Command{}, err
		}
		if profile == nil {
			return protocol.Command{}, denied
		}
		switch action.Kind {
		case ConnectionBrowse:
			return protocol.Command{Kind: "browse_database", Data: &protocol.CommandBrowseDatabasePayload{Profile: id, ExpectedRevision: profile.Revision, Database: action.Database}}, nil
		case ConnectionQuery:
			return protocol.Command{Kind: "query_database", Data: &protocol.CommandQueryDatabasePayload{ReadOnly: action.ReadOnly, Database: action.Database, Profile: id, ExpectedRevision: profile.Revision, Sql: action.Sql, RowLimit: action.RowLimit, TimeoutMs: timeout}}, nil
		}
		return protocol.Command{}, denied
	}
	if id, valid := dispatchValue[protocol.SshID](target.Data); valid && target.Kind == "ssh" {
		profile, err := readBody[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles WHERE id=?1", id.String())
		if err != nil {
			return protocol.Command{}, err
		}
		if profile == nil {
			return protocol.Command{}, denied
		}
		switch action.Kind {
		case ConnectionRun:
			if action.TimeoutMs == nil {
				timeout = 120_000
			}
			return protocol.Command{Kind: "run_ssh", Data: &protocol.CommandRunSshPayload{Profile: id, ExpectedRevision: profile.Revision, Command: action.Command, TimeoutMs: timeout}}, nil
		case ConnectionTransfer:
			return protocol.Command{Kind: "transfer_ssh", Data: &protocol.CommandTransferSshPayload{Profile: id, ExpectedRevision: profile.Revision, Transfer: protocol.SshTransfer{Worktree: run.Worktree, Path: action.Path, RemotePath: action.RemotePath, Direction: action.Direction}, TimeoutMs: timeout}}, nil
		}
	}
	return protocol.Command{}, denied
}

// ConnectionCreated reports whether this turn's package created the target
// through its own approved and completed save. An unbound conversation may
// keep using such a connection before it is shared with its project.
func ConnectionCreated(ctx context.Context, db *sql.Conn, turn protocol.TurnID, owner protocol.PluginReference, target protocol.ConnectionResource) (bool, error) {
	if _, err := activeAgentRun(ctx, db, turn); err != nil {
		return false, err
	}
	capture, err := readTurnCapture(ctx, db, turn)
	if err != nil {
		return false, err
	}
	if capture.Config.Resource != nil {
		return false, nil
	}
	rows, err := db.QueryContext(ctx, "SELECT r.body,r.result,a.binding FROM agent_approvals a JOIN turns t ON t.id=a.turn JOIN requests r ON r.id=a.request WHERE a.turn=?1 AND a.claimed=1 AND json_extract(a.body,'$.state')='approved' AND r.status='completed'", turn.String())
	if err != nil {
		return false, err
	}
	defer rows.Close()
	kind, identity, valid := connectionIdentity(target)
	for rows.Next() {
		var body, bound []byte
		var result string
		if err := rows.Scan(&body, &result, &bound); err != nil {
			return false, err
		}
		var binding *ApprovalBinding
		if err := json.Unmarshal(bound, &binding); err != nil {
			return false, err
		}
		if binding == nil || binding.Owner != owner {
			continue
		}
		var request protocol.Request
		if err := json.Unmarshal(body, &request); err != nil {
			return false, err
		}
		var outcome protocol.Result[protocol.Output, protocol.Fault]
		if err := json.Unmarshal([]byte(result), &outcome); err != nil {
			return false, fault(protocol.ErrorCodeInternal, "stored request outcome is invalid")
		}
		if !valid || outcome.Ok == nil {
			continue
		}
		switch kind {
		case "database":
			command, issued := dispatchValue[protocol.CommandSaveDatabasePayload](request.Command.Data)
			saved, completed := dispatchValue[protocol.DatabaseProfile](outcome.Ok.Data)
			if request.Command.Kind == "save_database" && issued && completed && command.ExpectedRevision == 0 &&
				command.Profile.ID.String() == identity && saved.ID.String() == identity && saved.Revision == 1 {
				return true, nil
			}
		case "ssh":
			command, issued := dispatchValue[protocol.CommandSaveSshPayload](request.Command.Data)
			saved, completed := dispatchValue[protocol.SshProfile](outcome.Ok.Data)
			if request.Command.Kind == "save_ssh" && issued && completed && command.ExpectedRevision == 0 &&
				command.Profile.ID.String() == identity && saved.ID.String() == identity && saved.Revision == 1 {
				return true, nil
			}
		}
	}
	return false, rows.Err()
}

// checkConnection admits a target the turn created itself, or one that was
// both captured at admission and is still permitted by the sharing decision.
func checkConnection(ctx context.Context, db *sql.Conn, turn protocol.TurnID, owner protocol.PluginReference, target protocol.ConnectionResource, captured []protocol.ConnectionResource) error {
	run, err := activeAgentRun(ctx, db, turn)
	if err != nil {
		return err
	}
	created, err := ConnectionCreated(ctx, db, turn, owner, target)
	if err != nil || created {
		return err
	}
	catalog, err := ReadConnectionCatalog(ctx, db, run.Session)
	if err != nil {
		return err
	}
	if containsConnection(captured, target) && containsConnection(catalog.Resources(), target) {
		return nil
	}
	return fault(protocol.ErrorCodePermissionDenied, "connection is not available to this turn")
}

// ConnectionEntry names one package's reference to a connection.
type ConnectionEntry struct {
	Package protocol.PluginReference
	Target  protocol.ConnectionResource
}

// ConnectionNames returns, per entry, the saved name of a connection the
// entry's package created in this turn, and nil for every other entry.
func ConnectionNames(ctx context.Context, db *sql.Conn, turn protocol.TurnID, entries []ConnectionEntry, captured []protocol.ConnectionResource) ([]*string, error) {
	names := make([]*string, 0, len(entries))
	for _, entry := range entries {
		created, err := ConnectionCreated(ctx, db, turn, entry.Package, entry.Target)
		if err != nil {
			return nil, err
		}
		if !created {
			names = append(names, nil)
			continue
		}
		if err := checkConnection(ctx, db, turn, entry.Package, entry.Target, captured); err != nil {
			return nil, err
		}
		var name *string
		if id, valid := dispatchValue[protocol.DatabaseID](entry.Target.Data); valid && entry.Target.Kind == "database" {
			profile, err := ReadDatabase(ctx, db, id)
			if err != nil {
				return nil, err
			}
			if profile != nil {
				name = &profile.Name
			}
		} else if id, valid := dispatchValue[protocol.SshID](entry.Target.Data); valid && entry.Target.Kind == "ssh" {
			profile, err := readBody[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles WHERE id=?1", id.String())
			if err != nil {
				return nil, err
			}
			if profile != nil {
				name = &profile.Name
			}
		}
		if name != nil && strings.TrimSpace(*name) == "" {
			name = nil
		}
		names = append(names, name)
	}
	return names, nil
}

// ConnectionStatus is one reachable target and whether its network database
// password is still missing.
type ConnectionStatus struct {
	Target          protocol.ConnectionResource
	MissingPassword bool
}

// ConnectionStatuses reports the targets the turn may reach, in request
// order. Targets outside the turn are omitted, not reported as faults.
func ConnectionStatuses(ctx context.Context, db *sql.Conn, turn protocol.TurnID, owner protocol.PluginReference, targets, captured []protocol.ConnectionResource) ([]ConnectionStatus, error) {
	statuses := []ConnectionStatus{}
	for _, target := range targets {
		if slices.ContainsFunc(statuses, func(status ConnectionStatus) bool { return sameConnection(status.Target, target) }) {
			continue
		}
		if err := checkConnection(ctx, db, turn, owner, target, captured); err != nil {
			var denied *protocol.Fault
			if errors.As(err, &denied) && denied.Code == protocol.ErrorCodePermissionDenied {
				continue
			}
			return nil, err
		}
		missing := false
		if id, valid := dispatchValue[protocol.DatabaseID](target.Data); valid && target.Kind == "database" {
			profile, err := ReadDatabase(ctx, db, id)
			if err != nil {
				return nil, err
			}
			if profile == nil {
				return nil, fault(protocol.ErrorCodeNotFound, "database connection does not exist")
			}
			if profile.Connection.Kind != "sqlite" {
				if err := db.QueryRowContext(ctx, "SELECT length(password)=0 FROM database_profiles WHERE id=?1", id.String()).Scan(&missing); err != nil {
					return nil, err
				}
			}
		}
		statuses = append(statuses, ConnectionStatus{Target: target, MissingPassword: missing})
	}
	return statuses, nil
}

// ConnectionInspection is an unverified host key observed while a captured
// SSH operation connected.
type ConnectionInspection struct {
	Profile          protocol.SshID
	ExpectedRevision uint64
	Key              protocol.SshHostKey
	Changed          bool
}

// ConnectionTrust builds the host-key consent an approval presents for a
// captured SSH operation.
func ConnectionTrust(ctx context.Context, db *sql.Conn, turn protocol.TurnID, owner protocol.PluginReference, inspection ConnectionInspection, captured []protocol.ConnectionResource) (protocol.SshTrust, error) {
	if err := checkConnection(ctx, db, turn, owner, sshResource(inspection.Profile), captured); err != nil {
		return protocol.SshTrust{}, err
	}
	profile, err := readBody[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles WHERE id=?1", inspection.Profile.String())
	if err != nil {
		return protocol.SshTrust{}, err
	}
	if profile == nil {
		return protocol.SshTrust{}, fault(protocol.ErrorCodeNotFound, "SSH connection does not exist")
	}
	if err := checkRevision(profile.Revision, inspection.ExpectedRevision); err != nil {
		return protocol.SshTrust{}, err
	}
	return protocol.SshTrust{Profile: profile.ID, ExpectedRevision: inspection.ExpectedRevision, Host: profile.Host, Port: profile.Port, Key: inspection.Key, Changed: inspection.Changed}, nil
}

// ValidateConnectionTrust accepts host-key consent only inside an approval of
// a captured SSH run or transfer, for the profile exactly as saved now and a
// key it does not already trust.
func ValidateConnectionTrust(ctx context.Context, db *sql.Conn, turn protocol.TurnID, binding *ApprovalBinding, trust protocol.SshTrust) error {
	denied := fault(protocol.ErrorCodePermissionDenied, "SSH host key consent is outside the captured operation")
	if binding == nil || binding.Operation == nil || *binding.Operation != protocol.ToolOperationRunSsh && *binding.Operation != protocol.ToolOperationTransferSsh {
		return denied
	}
	capture, err := readTurnCapture(ctx, db, turn)
	if err != nil {
		return err
	}
	if !slices.Contains(capture.Plugins, binding.Owner) {
		return denied
	}
	profile, err := readBody[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles WHERE id=?1", trust.Profile.String())
	if err != nil {
		return err
	}
	if profile == nil || profile.Revision != trust.ExpectedRevision || profile.Host != trust.Host || profile.Port != trust.Port ||
		trust.Changed != (profile.HostKey != nil) || profile.HostKey != nil && *profile.HostKey == trust.Key {
		return denied
	}
	return nil
}

// ConnectionOwner returns the running turn that owns a connection operation.
func ConnectionOwner(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationRun, error) {
	return activeAgentRun(ctx, db, turn)
}

// ConnectionBound returns the resource the turn's captured configuration is
// bound to, if any.
func ConnectionBound(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (*protocol.ConnectionResource, error) {
	capture, err := readTurnCapture(ctx, db, turn)
	return capture.Config.Resource, err
}
