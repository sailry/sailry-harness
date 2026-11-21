package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type AgentHistorySource struct {
	Caller  protocol.NodeID
	Session protocol.SessionID
	Turn    protocol.TurnID
}
type agentReferenceList struct {
	Text           *string `json:"text"`
	Since          *int64  `json:"since_ms"`
	Until          *int64  `json:"until_ms"`
	Before         *uint64 `json:"before"`
	Revision       *string `json:"revision"`
	IncludeCurrent bool    `json:"include_current"`
	Limit          *uint16 `json:"limit"`
}
type agentReferenceRead struct {
	Session       protocol.SessionID `json:"session"`
	Mode          *string            `json:"mode"`
	Before        *protocol.TurnID   `json:"before"`
	Turn          *protocol.TurnID   `json:"turn"`
	Revision      *uint64            `json:"revision"`
	BeforeEntry   *uint64            `json:"before_entry"`
	EntrySequence *uint64            `json:"entry_sequence"`
	Limit         *uint64            `json:"limit"`
}
type agentReferenceSearch struct {
	Session       protocol.SessionID `json:"session"`
	Text          string             `json:"text"`
	CaseSensitive bool               `json:"case_sensitive"`
	Before        *uint64            `json:"before"`
	Revision      *uint64            `json:"revision"`
	Limit         *uint16            `json:"limit"`
}

func ReadAgentReference(ctx context.Context, db *sql.Conn, node protocol.NodeID, source AgentHistorySource, kind string, arguments json.RawMessage) (json.RawMessage, error) {
	run, err := activeAgentRun(ctx, db, source.Turn)
	if err != nil {
		return nil, err
	}
	var caller []byte
	if err = db.QueryRowContext(ctx, "SELECT caller FROM turns WHERE id=?1", source.Turn.String()).Scan(&caller); err != nil {
		return nil, err
	}
	if run.Session != source.Session || string(caller) != string(source.Caller[:]) {
		return nil, fault(protocol.ErrorCodePermissionDenied, "history read does not belong to this invocation")
	}
	visible := func(target protocol.SessionID) error {
		project, err := referenceSessionProject(ctx, db, source.Session)
		if err != nil {
			return err
		}
		if err := CheckSessionAccess(ctx, db, target); err != nil {
			return err
		}
		targetProject, err := referenceSessionProject(ctx, db, target)
		if err != nil {
			return err
		}
		if !sameProject(targetProject, project) {
			return fault(protocol.ErrorCodeWrongTarget, "conversation belongs to another project")
		}
		return nil
	}
	switch kind {
	case "list_sessions":
		var options agentReferenceList
		if err = protocol.DecodeObject(arguments, &options, []string{}, true); err != nil {
			return nil, invalid("invalid session directory arguments")
		}
		session, err := ReadSession(ctx, db, source.Session)
		if err != nil {
			return nil, err
		}
		return readAgentDirectory(ctx, db, node, session, options)
	case "read_session":
		var options agentReferenceRead
		if err = protocol.DecodeObject(arguments, &options, []string{"session"}, true); err != nil {
			return nil, invalid("invalid session history arguments")
		}
		if err = visible(options.Session); err != nil {
			return nil, err
		}
		return readAgentReferenceHistory(ctx, db, options)
	case "search_session":
		var options agentReferenceSearch
		if err = protocol.DecodeObject(arguments, &options, []string{"session", "text"}, true); err != nil {
			return nil, invalid("invalid session search arguments")
		}
		if err = visible(options.Session); err != nil {
			return nil, err
		}
		if err = checkReferenceRevision(ctx, db, options.Session, options.Revision, options.Before != nil); err != nil {
			return nil, err
		}
		limit := uint16(20)
		if options.Limit != nil {
			limit = *options.Limit
		}
		page, e := searchAgentConversation(ctx, db, options.Session, protocol.ConversationSearchQuery{Text: options.Text, CaseSensitive: options.CaseSensitive, Before: options.Before, Limit: limit})
		if e != nil {
			return nil, e
		}
		return encode(page)
	}
	return nil, invalid("unknown session history query")
}

func referenceSessionProject(ctx context.Context, db *sql.Conn, session protocol.SessionID) (*protocol.ProjectID, error) {
	var project sql.NullString
	err := db.QueryRowContext(ctx, "SELECT project FROM sessions WHERE id=?1 AND state!='removed'", session.String()).Scan(&project)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, fault(protocol.ErrorCodeNotFound, "session does not exist on this Node")
	}
	if err != nil || !project.Valid {
		return nil, err
	}
	id, err := protocol.ParseProjectID(project.String)
	return &id, err
}

func sameProject(a, b *protocol.ProjectID) bool {
	return a == nil && b == nil || a != nil && b != nil && *a == *b
}
func checkReferenceRevision(ctx context.Context, db *sql.Conn, session protocol.SessionID, expected *uint64, required bool) error {
	if expected == nil && !required {
		return nil
	}
	_, err := readReferenceSnapshotRevision(ctx, db, session, expected, required)
	return err
}
func referenceCursorSession(ctx context.Context, db *sql.Conn, before uint64, project *protocol.ProjectID) error {
	if before == 0 || before > math.MaxInt64 {
		return invalid("session directory bounds are invalid")
	}
	var text string
	if err := db.QueryRowContext(ctx, "SELECT id FROM sessions WHERE rowid=?1 AND state!='removed'", before).Scan(&text); errors.Is(err, sql.ErrNoRows) {
		return fault(protocol.ErrorCodeNotFound, "session cursor is unavailable")
	} else if err != nil {
		return err
	}
	id, err := protocol.ParseSessionID(text)
	if err != nil {
		return err
	}
	session, err := ReadSession(ctx, db, id)
	if err != nil {
		return err
	}
	if !sameProject(session.Project, project) {
		return fault(protocol.ErrorCodeWrongTarget, "conversation belongs to another project")
	}
	return nil
}

// validateAgentReferences checks every reference of an input before its turn
// is admitted. captured names the packages of an already admitted turn, whose
// config and roster the session then carries; a new turn resolves the current
// packages.
func validateAgentReferences(ctx context.Context, db *sql.Conn, session protocol.Session, message protocol.ConversationInput, captured []protocol.PluginReference) error {
	var targets []string
	role := false
	for _, reference := range message.References {
		target, err := encode(reference.Target)
		if err != nil {
			return err
		}
		if strings.TrimSpace(reference.Label) == "" || len(reference.Label) > 512 || slices.Contains(targets, string(target)) {
			return invalid("references require distinct targets and bounded labels")
		}
		targets = append(targets, string(target))
		kind := reference.Target.Kind
		if session.Config.Resource != nil && session.Config.Assistant == nil && kind != "database" && kind != "ssh" && kind != "session" && kind != "attachment" {
			return fault(protocol.ErrorCodePermissionDenied, "connection conversations only accept connection references")
		}
		switch kind {
		case "plugin":
			name, _ := dispatchValue[string](reference.Target.Data)
			if err = validateReferencedPlugin(ctx, db, session, name, nil, captured); err != nil {
				return err
			}
		case "skill":
			skill, _ := dispatchValue[protocol.ConversationReferenceTargetSkillPayload](reference.Target.Data)
			if err = validateReferencedPlugin(ctx, db, session, skill.Package, &skill.Name, captured); err != nil {
				return err
			}
		case "ssh":
			id, _ := dispatchValue[protocol.SshID](reference.Target.Data)
			available, err := ReadConnectionCatalog(ctx, db, session.ID)
			if err != nil {
				return err
			}
			if !slices.ContainsFunc(available.Ssh, func(profile protocol.SshProfile) bool { return profile.ID == id }) {
				return fault(protocol.ErrorCodePermissionDenied, "referenced SSH connection is not available to this conversation")
			}
		case "database":
			selected, _ := dispatchValue[protocol.ConversationReferenceTargetDatabasePayload](reference.Target.Data)
			available, err := ReadConnectionCatalog(ctx, db, session.ID)
			if err != nil {
				return err
			}
			if !slices.ContainsFunc(available.Databases, func(profile protocol.DatabaseProfile) bool { return profile.ID == selected.Connection }) {
				return fault(protocol.ErrorCodePermissionDenied, "referenced database is not available to this conversation")
			}
			bounded := func(name string) bool { return name != "" && len(name) <= 256 }
			if selected.Database != nil && !bounded(*selected.Database) ||
				selected.Table != nil && (selected.Database == nil || !bounded(selected.Table.Schema) || !bounded(selected.Table.Name)) {
				return invalid("invalid database reference")
			}
		case "file", "directory":
			path, _ := dispatchValue[string](reference.Target.Data)
			if err = core.CheckFilePath(path); err != nil {
				return err
			}
		case "agent":
			selected, _ := dispatchValue[protocol.RoleReference](reference.Target.Data)
			if role || !slices.ContainsFunc(session.Roles.Profiles, func(profile protocol.RoleProfile) bool {
				return profile.ID == selected.ID && profile.Revision == selected.Revision
			}) {
				return invalid("select one role from the frozen session roster")
			}
			role = true
		case "session":
			id, _ := dispatchValue[protocol.SessionID](reference.Target.Data)
			target, err := ReadSession(ctx, db, id)
			if err != nil {
				return err
			}
			if err := CheckSessionAccess(ctx, db, id); err != nil {
				return err
			}
			if !sameProject(target.Project, session.Project) {
				return fault(protocol.ErrorCodeWrongTarget, "conversation belongs to another project")
			}
		case "attachment":
			id, _ := dispatchValue[protocol.AttachmentID](reference.Target.Data)
			if !slices.Contains(message.Attachments, id) {
				return invalid("attachment references require an attachment of the same input")
			}
		}
	}
	return nil
}

// validateReferencedPlugin admits a plugin or skill reference only for a
// package the turn freezes that an agent can use.
func validateReferencedPlugin(ctx context.Context, db *sql.Conn, session protocol.Session, name string, skill *string, captured []protocol.PluginReference) error {
	references := captured
	if captured == nil {
		var err error
		if references, err = captureTurnPlugins(ctx, db, session.Config.Assistant); err != nil {
			return err
		}
	}
	index := slices.IndexFunc(references, func(reference protocol.PluginReference) bool { return reference.Name == name })
	if index < 0 {
		return invalid("referenced plugin is disabled or unavailable")
	}
	var scope protocol.PluginScope
	var skills []protocol.PluginSkill
	if info, err := ReadPluginVersion(ctx, db, references[index]); err == nil {
		skills = info.Skills
		if info.Extension != nil {
			scope = info.Extension.Scope
		}
	} else {
		// Before the first inventory a turn freezes the bundled packages.
		catalog, e := operations.Bundled()
		if e != nil {
			return e
		}
		bundled := slices.IndexFunc(catalog.Packages, func(value operations.Package) bool {
			return value.Reference.Name == references[index].Name && value.Reference.Digest == references[index].Digest
		})
		if bundled < 0 {
			return err
		}
		scope, skills = catalog.Packages[bundled].Scope, catalog.Packages[bundled].Skills
	}
	if scope == protocol.PluginScopeDesktop {
		return invalid("desktop-only plugins cannot be referenced by an agent")
	}
	if skill != nil && !slices.ContainsFunc(skills, func(candidate protocol.PluginSkill) bool { return candidate.Name == *skill }) {
		return invalid("referenced skill is unavailable in this plugin version")
	}
	return nil
}
