package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// PreparedPaths contains the observations and effects made outside SQL
// transactions: the canonical directory, and whether a project removal already
// closed one of the project's terminals.
type PreparedPaths struct {
	Path            string
	TerminalsClosed bool
}

func PreparePaths(command protocol.Command) (PreparedPaths, error) {
	var path string
	switch value := command.Data.(type) {
	case *protocol.ProjectsDraft:
		if value.Source.Kind != "local" {
			return PreparedPaths{}, fault(protocol.ErrorCodeNotConfigured, "project cloning is unavailable")
		}
		if err := validateProject(value.Name, value.Path, value.Appearance); err != nil {
			return PreparedPaths{}, err
		}
		path = value.Path
	case *protocol.CommandRegisterProjectPayload:
		if err := validateProject(value.Name, value.Path, protocol.DefaultProjectsAppearance()); err != nil {
			return PreparedPaths{}, err
		}
		path = value.Path
	case *protocol.CommandUpdateProjectPayload:
		if err := validateProject(value.Name, value.Path, value.Appearance); err != nil {
			return PreparedPaths{}, err
		}
		if value.Path == value.Expected.Path {
			return PreparedPaths{Path: value.Path}, nil
		}
		path = value.Path
	case *protocol.CommandRegisterWorktreePayload:
		if !filepath.IsAbs(value.Path) {
			return PreparedPaths{}, invalid("worktree path must be absolute")
		}
		path = value.Path
	default:
		return PreparedPaths{}, nil
	}
	canonical, err := filepath.EvalSymlinks(path)
	if err != nil {
		return PreparedPaths{}, invalid("project directory is not accessible on this Node")
	}
	info, err := os.Stat(canonical)
	if err != nil {
		return PreparedPaths{}, invalid("project directory is not accessible on this Node")
	}
	if !info.IsDir() {
		return PreparedPaths{}, invalid("project path is not a directory")
	}
	if !utf8.ValidString(canonical) {
		return PreparedPaths{}, invalid("project path is not UTF-8")
	}
	return PreparedPaths{Path: canonical}, nil
}

func validateProject(name, path string, appearance protocol.ProjectsAppearance) error {
	if strings.TrimSpace(name) == "" || len(name) > 128 || len(path) > 4096 || !filepath.IsAbs(path) {
		return invalid("an absolute directory and a nonempty project name are required")
	}
	return validateAppearance(appearance)
}
func validateAppearance(appearance protocol.ProjectsAppearance) error {
	icons := strings.Fields("folder code terminal globe server database package game book finance device education writing tags music media design health nature business analytics profile fitness law audio travel tools science ai favorite")
	colors := strings.Fields("none blue indigo violet magenta red orange amber green teal cyan")
	if !slices.Contains(icons, appearance.Icon) || !slices.Contains(colors, appearance.Color) {
		return invalid("project appearance is invalid")
	}
	return nil
}

func ReadProjects(ctx context.Context, db *sql.Conn) ([]protocol.Project, error) {
	rows, err := db.QueryContext(ctx, "SELECT id,name,path,appearance FROM projects WHERE id NOT IN (SELECT project FROM removed_projects) ORDER BY rowid")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	values := []protocol.Project{}
	for rows.Next() {
		var value protocol.Project
		var id string
		var body []byte
		if err := rows.Scan(&id, &value.Name, &value.Path, &body); err != nil {
			return nil, err
		}
		value.ID, err = protocol.ParseProjectID(id)
		if err != nil {
			return nil, err
		}
		if err := json.Unmarshal(body, &value.Appearance); err != nil {
			return nil, err
		}
		values = append(values, value)
	}
	return values, rows.Err()
}

func checkProject(ctx context.Context, db *sql.Conn, expected protocol.Project) error {
	var actual protocol.Project
	var id string
	var body []byte
	err := db.QueryRowContext(ctx, "SELECT id,name,path,appearance FROM projects WHERE id=?1", expected.ID.String()).Scan(&id, &actual.Name, &actual.Path, &body)
	if errors.Is(err, sql.ErrNoRows) {
		return fault(protocol.ErrorCodeRevisionConflict, "project changed; reload before saving")
	}
	if err != nil {
		return err
	}
	actual.ID, err = protocol.ParseProjectID(id)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(body, &actual.Appearance); err != nil {
		return err
	}
	if !reflect.DeepEqual(actual, expected) {
		return fault(protocol.ErrorCodeRevisionConflict, "project changed; reload before saving")
	}
	var removed bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM removed_projects WHERE project=?1)", expected.ID.String()).Scan(&removed); err != nil {
		return err
	}
	if removed {
		return fault(protocol.ErrorCodeNotFound, "project was removed")
	}
	return nil
}

func createProject(ctx context.Context, db *sql.Conn, input protocol.ProjectsDraft, prepared PreparedPaths) (protocol.Output, []protocol.Event, error) {
	value := protocol.Project{ID: protocol.NewProjectID(), Name: strings.TrimSpace(input.Name), Path: prepared.Path, Appearance: input.Appearance}
	var removedID string
	err := db.QueryRowContext(ctx, "SELECT p.id FROM projects p JOIN removed_projects r ON r.project=p.id WHERE p.path=?1", value.Path).Scan(&removedID)
	if err == nil {
		value.ID, err = protocol.ParseProjectID(removedID)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM removed_projects WHERE project=?1", removedID); err != nil {
			return protocol.Output{}, nil, err
		}
		body, err := encode(value.Appearance)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err := db.ExecContext(ctx, "UPDATE projects SET name=?2,appearance=?3 WHERE id=?1", removedID, value.Name, body); err != nil {
			return protocol.Output{}, nil, err
		}
	} else {
		if !errors.Is(err, sql.ErrNoRows) {
			return protocol.Output{}, nil, err
		}
		var exists bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM projects WHERE path=?1)", value.Path).Scan(&exists); err != nil {
			return protocol.Output{}, nil, err
		}
		if exists {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "project directory is already registered")
		}
		body, err := encode(value.Appearance)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO projects(id,name,path,appearance) VALUES(?1,?2,?3,?4)", value.ID.String(), value.Name, value.Path, body); err != nil {
			return protocol.Output{}, nil, err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,?3,1)", protocol.NewWorktreeID().String(), value.ID.String(), value.Path); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	return protocol.Output{Kind: "project", Data: value}, []protocol.Event{{Kind: "project_registered", Data: value}}, nil
}

func updateProject(ctx context.Context, db *sql.Conn, input protocol.CommandUpdateProjectPayload, prepared PreparedPaths) (protocol.Output, []protocol.Event, error) {
	if err := checkProject(ctx, db, input.Expected); err != nil {
		return protocol.Output{}, nil, err
	}
	if prepared.Path != input.Expected.Path {
		var duplicate, busy bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM worktrees WHERE path=?1)", prepared.Path).Scan(&duplicate); err != nil {
			return protocol.Output{}, nil, err
		}
		if duplicate {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "directory is already registered")
		}
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision JOIN worktrees w ON w.id=r.worktree WHERE w.project=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))", input.Expected.ID.String()).Scan(&busy); err != nil {
			return protocol.Output{}, nil, err
		}
		if busy {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeBusy, "project has active or queued turns")
		}
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM terminals t JOIN worktrees w ON w.id=t.worktree WHERE w.project=?1 AND w.main=1 AND json_extract(t.body,'$.status')!='closed')", input.Expected.ID.String()).Scan(&busy); err != nil {
			return protocol.Output{}, nil, err
		}
		if busy {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeBusy, "project checkout has an open or pending resource")
		}
		if _, err := db.ExecContext(ctx, "UPDATE worktrees SET path=?2 WHERE project=?1 AND main=1", input.Expected.ID.String(), prepared.Path); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	value := protocol.Project{ID: input.Expected.ID, Name: strings.TrimSpace(input.Name), Path: prepared.Path, Appearance: input.Appearance}
	body, err := encode(value.Appearance)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	_, err = db.ExecContext(ctx, "UPDATE projects SET name=?2,path=?3,appearance=?4 WHERE id=?1", value.ID.String(), value.Name, value.Path, body)
	return protocol.Output{Kind: "project", Data: value}, []protocol.Event{{Kind: "project_changed", Data: value}}, err
}

// ProjectRemoval lists what the Node closes before a removal is recorded.
type ProjectRemoval struct {
	Terminals []protocol.TerminalInfo
	Sessions  []protocol.SessionID
}

// PlanProjectRemoval validates a removal and lists the project's terminals
// that are not closed and its sessions, whose background commands stop. A
// stale or busy removal fails here, before anything is closed.
func PlanProjectRemoval(ctx context.Context, db *sql.Conn, expected protocol.Project) (ProjectRemoval, error) {
	if err := checkProjectRemoval(ctx, db, expected); err != nil {
		return ProjectRemoval{}, err
	}
	terminals, err := openProjectTerminals(ctx, db, expected.ID)
	if err != nil {
		return ProjectRemoval{}, err
	}
	rows, err := db.QueryContext(ctx, "SELECT id FROM sessions WHERE project=?1 ORDER BY rowid", expected.ID.String())
	if err != nil {
		return ProjectRemoval{}, err
	}
	defer rows.Close()
	plan := ProjectRemoval{Terminals: terminals}
	for rows.Next() {
		var text string
		if err := rows.Scan(&text); err != nil {
			return ProjectRemoval{}, err
		}
		id, err := protocol.ParseSessionID(text)
		if err != nil {
			return ProjectRemoval{}, err
		}
		plan.Sessions = append(plan.Sessions, id)
	}
	return plan, rows.Err()
}

func checkProjectRemoval(ctx context.Context, db *sql.Conn, expected protocol.Project) error {
	if err := checkProject(ctx, db, expected); err != nil {
		return err
	}
	var busy bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN sessions s ON s.id=t.session WHERE s.project=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))", expected.ID.String()).Scan(&busy); err != nil {
		return err
	}
	if busy {
		return &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "project has pending tasks", Details: &protocol.FaultDetails{Kind: "project_removal", Data: protocol.FaultDetailsProjectRemovalPayload{TasksActive: true}}}
	}
	return nil
}

func openProjectTerminals(ctx context.Context, db *sql.Conn, project protocol.ProjectID) ([]protocol.TerminalInfo, error) {
	return readBodies[protocol.TerminalInfo](ctx, db, "SELECT t.body FROM terminals t JOIN worktrees w ON w.id=t.worktree WHERE w.project=?1 AND json_extract(t.body,'$.status')!='closed' ORDER BY t.rowid", project.String())
}

// removeProject records a removal after the Node closed the project's running
// terminals. Records left by an earlier process have no terminal behind them
// and are closed here. A failure names the terminals the Node already closed.
func removeProject(ctx context.Context, db *sql.Conn, expected protocol.Project, terminalsClosed bool) (protocol.Output, []protocol.Event, error) {
	output, changes, err := recordProjectRemoval(ctx, db, expected)
	var failure *protocol.Fault
	if terminalsClosed && errors.As(err, &failure) {
		details := protocol.FaultDetailsProjectRemovalPayload{TerminalsMayBeClosed: true}
		if failure.Details != nil {
			if previous, ok := failure.Details.Data.(protocol.FaultDetailsProjectRemovalPayload); ok {
				details.TasksActive = previous.TasksActive
			}
		}
		marked := *failure
		marked.Details = &protocol.FaultDetails{Kind: "project_removal", Data: details}
		err = &marked
	}
	return output, changes, err
}

func recordProjectRemoval(ctx context.Context, db *sql.Conn, expected protocol.Project) (protocol.Output, []protocol.Event, error) {
	if err := checkProjectRemoval(ctx, db, expected); err != nil {
		return protocol.Output{}, nil, err
	}
	terminals, err := openProjectTerminals(ctx, db, expected.ID)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	var changes []protocol.Event
	for _, terminal := range terminals {
		if terminal.Status.Kind == "running" {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeBusy, "project still has open terminals")
		}
		terminal.Status, terminal.Owner = protocol.TerminalStatus{Kind: "closed"}, nil
		terminal.Revision++
		closed, err := SaveTerminal(ctx, db, terminal, false)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		changes = append(changes, closed...)
	}
	_, err = db.ExecContext(ctx, "INSERT INTO removed_projects(project) VALUES(?1)", expected.ID.String())
	return protocol.Output{Kind: "project_removed", Data: protocol.OutputProjectRemovedPayload{ID: expected.ID}}, append(changes, protocol.Event{Kind: "project_removed", Data: protocol.EventProjectRemovedPayload{ID: expected.ID}}), err
}

func ReadWorktrees(ctx context.Context, db *sql.Conn) ([]protocol.Worktree, error) {
	rows, err := db.QueryContext(ctx, "SELECT id,project,path,main FROM worktrees WHERE id NOT IN (SELECT worktree FROM removed_worktrees) ORDER BY rowid")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	values := []protocol.Worktree{}
	for rows.Next() {
		var value protocol.Worktree
		var id string
		var project sql.NullString
		if err := rows.Scan(&id, &project, &value.Path, &value.Main); err != nil {
			return nil, err
		}
		value.ID, err = protocol.ParseWorktreeID(id)
		if err != nil {
			return nil, err
		}
		if project.Valid {
			id, err := protocol.ParseProjectID(project.String)
			if err != nil {
				return nil, err
			}
			value.Project = &id
		}
		values = append(values, value)
	}
	return values, rows.Err()
}

func registerWorktree(ctx context.Context, db *sql.Conn, input protocol.CommandRegisterWorktreePayload, prepared PreparedPaths) (protocol.Output, []protocol.Event, error) {
	var exists bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND id NOT IN (SELECT project FROM removed_projects))", input.Project.String()).Scan(&exists); err != nil {
		return protocol.Output{}, nil, err
	}
	if !exists {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "project does not exist on this Node")
	}
	value := protocol.Worktree{ID: protocol.NewWorktreeID(), Project: &input.Project, Path: prepared.Path}
	var project sql.NullString
	var id string
	var removed bool
	err := db.QueryRowContext(ctx, "SELECT id,project,main,EXISTS(SELECT 1 FROM removed_worktrees WHERE worktree=w.id) FROM worktrees w WHERE path=?1", value.Path).Scan(&id, &project, &value.Main, &removed)
	if err == nil {
		value.ID, err = protocol.ParseWorktreeID(id)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if !project.Valid || project.String != input.Project.String() {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeConflict, "worktree is registered to another project")
		}
		if !removed {
			return protocol.Output{Kind: "worktree", Data: value}, nil, nil
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM removed_worktrees WHERE worktree=?1", value.ID.String()); err != nil {
			return protocol.Output{}, nil, err
		}
		value.Main = false
	} else {
		if !errors.Is(err, sql.ErrNoRows) {
			return protocol.Output{}, nil, err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,?3,0)", value.ID.String(), input.Project.String(), value.Path); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	return protocol.Output{Kind: "worktree", Data: value}, []protocol.Event{{Kind: "worktree_registered", Data: value}}, nil
}

// ValidateProjectDraft checks the request before native clone effects begin.
func ValidateProjectDraft(value protocol.ProjectsDraft) error {
	return validateProject(value.Name, value.Path, value.Appearance)
}
