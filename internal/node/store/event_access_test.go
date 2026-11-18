package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestEventAudiencesRetainCursorsAndRecheckProjectAccess(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		other, err := CreateUser(ctx, db, "Event recipient")
		if err != nil {
			return err
		}
		owned, recipient := WithUser(ctx, owner), WithUser(ctx, other)
		create := func(visibility protocol.ProjectVisibility) (protocol.Project, protocol.Session, error) {
			draft := protocol.DefaultProjectsDraft()
			draft.Name, draft.Path, draft.Visibility = "Event project", t.TempDir(), visibility
			out, _, err := createProject(owned, db, owner.ID, draft, PreparedPaths{Path: draft.Path})
			if err != nil {
				return protocol.Project{}, protocol.Session{}, err
			}
			project := out.Data.(protocol.Project)
			provider := providerFixture()
			key := protocol.NewSecret("event-session-fixture-key")
			out, _, err = ExecuteProviderCommand(owned, db, node, protocol.NewRequest(node, protocol.Command{Kind: "save_provider", Data: &protocol.CommandSaveProviderPayload{Provider: provider, Secret: &key}}), nil)
			if err != nil {
				return project, protocol.Session{}, err
			}
			saved := out.Data.(protocol.ConversationProvider)
			config := protocol.DefaultSessionConfig()
			config.Provider, config.Model, config.Credential = saved.ID, saved.DefaultModel, saved.Credential
			config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionProject
			out, _, err = createSession(owned, db, node, t.TempDir(), protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config})
			if err != nil {
				return project, protocol.Session{}, err
			}
			return project, out.Data.(protocol.Session), nil
		}
		_, private, err := create(protocol.ProjectVisibilityPrivate)
		if err != nil {
			return err
		}
		project, shared, err := create(protocol.ProjectVisibilityShared)
		if err != nil {
			return err
		}
		events, err := RecordEvents(owned, db, node, []protocol.Event{
			{Kind: "plugin_values_changed", Data: protocol.EventPluginValuesChangedPayload{Name: "personal"}},
			{Kind: "plugin_values_changed", Data: protocol.EventPluginValuesChangedPayload{Name: "shared", Project: &project.ID}},
			{Kind: "sessions_reordered", Data: []protocol.SessionID{private.ID, shared.ID}},
			{Kind: "knowledge_collections_changed"},
		})
		if err != nil {
			return err
		}
		through, page, err := EventPage(recipient, db, node, 0, 0)
		if err != nil {
			return err
		}
		if len(page) != 4 || through != events[3].Cursor || page[0].Event.Kind != "cursor" || page[1].Event.Kind != "plugin_values_changed" || page[3].Event.Kind != "projection_invalidated" {
			return fmt.Errorf("incorrect filtered event page: %+v", page)
		}
		for index := range page {
			if page[index].Cursor != events[index].Cursor {
				return fmt.Errorf("filtered event cursor is discontinuous")
			}
		}
		ids := page[2].Event.Data.([]protocol.SessionID)
		if len(ids) != 1 || ids[0] != shared.ID {
			return fmt.Errorf("mixed audience batch leaked private identifiers: %v", ids)
		}
		if _, err = db.ExecContext(ctx, "UPDATE projects SET visibility='private' WHERE id=?1", project.ID.String()); err != nil {
			return err
		}
		buffered, err := RefreshEventAccess(recipient, db, page[1])
		if err != nil || buffered.Cursor != page[1].Cursor || buffered.Event.Kind != "cursor" {
			return fmt.Errorf("buffered shared event survived revocation: %+v: %v", buffered, err)
		}
		if err = retainResourceAudience(owned, db, "session", private.ID.String()); err != nil {
			return err
		}
		if _, err = db.ExecContext(ctx, "UPDATE sessions SET state='removed' WHERE id=?1", private.ID.String()); err != nil {
			return err
		}
		removed, err := RecordEvents(owned, db, node, []protocol.Event{{Kind: "sessions_removed", Data: []protocol.SessionID{private.ID}}})
		if err != nil {
			return err
		}
		filtered, err := RefreshEventAccess(recipient, db, removed[0])
		if err != nil || filtered.Event.Kind != "cursor" {
			return fmt.Errorf("private deletion leaked: %+v: %v", filtered, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
