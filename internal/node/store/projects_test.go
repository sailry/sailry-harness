package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNewProjectsDefaultSharedAndPreservePrivate(t *testing.T) {
	t.Parallel()
	profile, node, storage := schemaFixture(t)
	ctx := t.Context()
	var private protocol.Project
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		other, err := CreateUser(ctx, db, "Other project user")
		if err != nil {
			return err
		}
		for _, kind := range []string{"create_project", "register_project"} {
			for _, visibility := range []protocol.ProjectVisibility{"", protocol.ProjectVisibilityShared, protocol.ProjectVisibilityPrivate} {
				path := t.TempDir()
				var payload any = &protocol.CommandRegisterProjectPayload{Name: "New", Path: path, Visibility: visibility}
				if kind == "create_project" {
					payload = &protocol.ProjectsDraft{Name: "New", Path: path, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"}, Visibility: visibility}
				}
				request := protocol.NewRequest(node, protocol.Command{Kind: kind, Data: payload})
				out, _, err := ExecuteCommand(ctx, db, node, node, profile, request, PreparedPaths{Path: path})
				if err != nil {
					return err
				}
				project := out.Data.(protocol.Project)
				want := visibility
				if want == "" {
					want = protocol.ProjectVisibilityShared
				}
				if project.Visibility != want || project.Owner == nil || *project.Owner != owner.ID {
					return fmt.Errorf("%s visibility %q created %+v", kind, visibility, project)
				}
				access := CheckProjectAccess(WithUser(ctx, other), db, project.ID, false)
				if (access == nil) != (want == protocol.ProjectVisibilityShared) {
					return fmt.Errorf("%s visibility %q access: %v", kind, want, access)
				}
				if visibility == protocol.ProjectVisibilityPrivate {
					private = project
				}
			}
		}
		// Re-registering a removed project restores the stored record, not a new draft.
		if _, _, err := recordProjectRemoval(ctx, db, private); err != nil {
			return err
		}
		draft := protocol.DefaultProjectsDraft()
		draft.Name, draft.Path = private.Name, private.Path
		out, _, err := createProject(ctx, db, owner.ID, draft, PreparedPaths{Path: private.Path})
		if err != nil {
			return err
		}
		restored := out.Data.(protocol.Project)
		if restored.ID != private.ID || restored.Visibility != protocol.ProjectVisibilityPrivate {
			return fmt.Errorf("restored private project changed: %+v", restored)
		}
		// Editing an older private projection with no visibility field stays private.
		expected := private
		expected.Visibility = ""
		_, _, err = updateProject(ctx, db, protocol.CommandUpdateProjectPayload{Expected: expected, Name: private.Name, Path: private.Path, Appearance: private.Appearance}, PreparedPaths{Path: private.Path})
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	reopened := reopenSchemaFixture(t, profile, node)
	if err := reopened.Node.Read(ctx, func(db *sql.Conn) error {
		var visibility protocol.ProjectVisibility
		if err := db.QueryRowContext(ctx, "SELECT visibility FROM projects WHERE id=?1", private.ID.String()).Scan(&visibility); err != nil {
			return err
		}
		if visibility != protocol.ProjectVisibilityPrivate {
			return fmt.Errorf("stored private project became %q on reopen", visibility)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
