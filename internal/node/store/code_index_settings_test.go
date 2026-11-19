package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCodeIndexProjectSettings(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		initial, err := ReadCodeIndexSettings(ctx, db)
		if err != nil || initial != protocol.DefaultCodeIndexSettings() {
			return fmt.Errorf("default code index settings = %+v, %v", initial, err)
		}
		view, err := ViewCodeIndexSettings(ctx, db)
		if err != nil || !view.Enabled || view.PersistentProjects == nil || len(*view.PersistentProjects) != 0 {
			return fmt.Errorf("default settings do not advertise project indexing: %+v, %v", view, err)
		}
		local, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		admin, err := CreateUser(ctx, db, "Code index administrator")
		if err != nil {
			return err
		}
		member, err := CreateUser(ctx, db, "Code index member")
		if err != nil {
			return err
		}
		project := func(name string, visibility protocol.ProjectVisibility) (protocol.ProjectID, error) {
			draft := protocol.DefaultProjectsDraft()
			draft.Name, draft.Path, draft.Visibility = name, t.TempDir(), visibility
			out, _, err := createProject(ctx, db, local.ID, draft, PreparedPaths{Path: draft.Path})
			if err != nil {
				return protocol.ProjectID{}, err
			}
			return out.Data.(protocol.Project).ID, nil
		}
		private, err := project("Private", protocol.ProjectVisibilityPrivate)
		if err != nil {
			return err
		}
		shared, err := project("Shared", protocol.ProjectVisibilityShared)
		if err != nil {
			return err
		}
		save := func(who context.Context, revision uint64, enabled bool, projects *[]protocol.ProjectID) (protocol.CodeIndexSettings, error) {
			return SaveCodeIndexSettings(who, db, protocol.CodeIndexSettings{
				Revision: revision, Enabled: enabled, PersistentProjects: projects,
			}, revision)
		}
		matches := func(value protocol.CodeIndexSettings, projects ...protocol.ProjectID) bool {
			return value.PersistentProjects != nil && slices.Equal(*value.PersistentProjects, projects)
		}
		selected := []protocol.ProjectID{private, shared}
		got, err := save(ctx, 0, true, &selected)
		if err != nil || got.Revision != 1 || !matches(got, private, shared) {
			return fmt.Errorf("selected project settings = %+v, %v", got, err)
		}
		got, err = save(ctx, 1, false, nil)
		if err != nil || got.Revision != 2 || got.Enabled || !matches(got, private, shared) {
			return fmt.Errorf("legacy machine switch cleared selections: %+v, %v", got, err)
		}
		adminCtx := WithUser(ctx, admin)
		got, err = ViewCodeIndexSettings(adminCtx, db)
		if err != nil || !matches(got, shared) {
			return fmt.Errorf("administrator sees incorrect project selections: %+v, %v", got, err)
		}
		got, err = ViewCodeIndexSettings(WithUser(ctx, member), db)
		if err != nil || got.Enabled || got.PersistentProjects != nil {
			return fmt.Errorf("member sees incorrect code tool settings: %+v, %v", got, err)
		}
		for _, ids := range [][]protocol.ProjectID{{private}, {shared, shared}, {protocol.NewProjectID()}} {
			_, err := save(adminCtx, 2, true, &ids)
			var failure *protocol.Fault
			if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeInvalidRequest {
				return fmt.Errorf("invalid selections %v returned %v", ids, err)
			}
		}
		var clear []protocol.ProjectID
		got, err = save(adminCtx, 2, true, &clear)
		if err != nil || got.Revision != 3 || !got.Enabled || !matches(got) {
			return fmt.Errorf("explicit clear did not clear visible selections: %+v, %v", got, err)
		}
		got, err = ReadCodeIndexSettings(ctx, db)
		if err != nil || !matches(got, private) {
			return fmt.Errorf("clearing visible selections affected a private project: %+v, %v", got, err)
		}
		_, err = save(ctx, 2, true, &clear)
		var failure *protocol.Fault
		if !errors.As(err, &failure) || failure.Code != protocol.ErrorCodeRevisionConflict {
			return fmt.Errorf("stale revision returned %v", err)
		}
		got, err = save(ctx, 3, true, &clear)
		if err != nil || got.Revision != 4 || !matches(got) {
			return fmt.Errorf("owner could not clear their selection: %+v, %v", got, err)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestCodeIndexSettingsProjectListJSON(t *testing.T) {
	for _, field := range []string{"", `,"persistent_projects":[]`} {
		expected := `{"revision":0,"enabled":true` + field + "}"
		var settings protocol.CodeIndexSettings
		if err := json.Unmarshal([]byte(expected), &settings); err != nil {
			t.Fatal(err)
		}
		if (settings.PersistentProjects == nil) != (field == "") {
			t.Fatalf("incorrect project list presence in %s", expected)
		}
		body, err := json.Marshal(settings)
		if err != nil || string(body) != expected {
			t.Fatalf("round trip of %s returned %s, %v", expected, body, err)
		}
	}
}
