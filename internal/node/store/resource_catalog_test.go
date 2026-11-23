package store

import (
	"database/sql"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestResourceCatalogSurvivesRestartAndSeparatesAccessPrincipals(t *testing.T) {
	t.Parallel()
	profile, entry, storage := schemaFixture(t)
	source := protocol.NodeID{99}
	var owners []protocol.User
	if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		if err := PairLinkPeer(t.Context(), db, source, link.DirectionHost); err != nil {
			return err
		}
		local, err := LocalUser(t.Context(), db)
		if err != nil {
			return err
		}
		other, err := CreateUser(t.Context(), db, "Another access identity")
		owners = []protocol.User{local, other}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	projects := []protocol.Project{
		{ID: protocol.NewProjectID(), Name: "Owner project", Path: "/srv/owner", Visibility: protocol.ProjectVisibilityPrivate},
		{ID: protocol.NewProjectID(), Name: "Other project", Path: "/srv/other", Visibility: protocol.ProjectVisibilityPrivate},
	}
	confirmed := uint64(1700000000000)
	for index, user := range owners {
		ctx := WithUser(t.Context(), user)
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			collection := protocol.DefaultKnowledgeCollection()
			collection.Node, collection.ID, collection.Owner.Kind = source, protocol.NewKnowledgeID(), "controller"
			collection.Definition.Name = "Directory summary"
			collection.Definition.Source = protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: "/srv/notes"}}
			canary := "must-not-persist-index-recipe"
			collection.Definition.Model, collection.Definition.Globs = &canary, []string{canary}
			collection.Progress.Model = &canary
			collection.Progress.Failures = []protocol.KnowledgeFailure{{Path: canary}}
			value := protocol.ResourceSource{Node: source, Route: protocol.RouteKey{Entry: entry, Target: source},
				Projects: []protocol.Project{projects[index]}, Knowledge: []protocol.KnowledgeCollection{collection},
				Revision: 47, LastSyncedAtMs: &confirmed}
			return SaveResourceCatalogSource(ctx, db, entry, value)
		}); err != nil {
			t.Fatal(err)
		}
	}
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	reopened := reopenSchemaFixture(t, profile, entry)
	for index, user := range owners {
		ctx := WithUser(t.Context(), user)
		if err := reopened.Node.Read(ctx, func(db *sql.Conn) error {
			values, err := ReadResourceCatalogSources(ctx, db, entry)
			if err != nil {
				return err
			}
			if len(values) != 1 || len(values[0].Projects) != 1 || values[0].Projects[0].ID != projects[index].ID || values[0].Revision != 47 || values[0].LastSyncedAtMs == nil || *values[0].LastSyncedAtMs != confirmed {
				return fmt.Errorf("directory crossed access identities or lost its confirmed version: %+v", values)
			}
			var body string
			if err := db.QueryRowContext(ctx, "SELECT body FROM resource_catalog_sources WHERE user=?1", user.ID.String()).Scan(&body); err != nil {
				return err
			}
			if strings.Contains(body, "must-not-persist") {
				return fmt.Errorf("directory retained an indexing recipe or diagnostic: %s", body)
			}
			if _, err := ReadResourceCatalogSources(ctx, db, protocol.NodeID{98}); err == nil {
				return fmt.Errorf("another controller entry reused this entry's cached route")
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
	if err := reopened.Node.Write(t.Context(), func(db *sql.Conn) error {
		if err := SetLinkTrust(t.Context(), db, source, false); err != nil {
			return err
		}
		var remaining int
		if err := db.QueryRowContext(t.Context(), "SELECT count(*) FROM resource_catalog_sources").Scan(&remaining); err != nil {
			return err
		}
		if remaining != 0 {
			return fmt.Errorf("removing the source retained %d access-principal caches", remaining)
		}
		stale := protocol.ResourceSource{Node: source, Route: protocol.RouteKey{Entry: entry, Target: source}, Projects: []protocol.Project{projects[0]}}
		if err := SaveResourceCatalogSource(t.Context(), db, entry, stale); err == nil {
			return fmt.Errorf("an in-flight source read recreated a removed connection cache")
		}
		if err := PairLinkPeer(t.Context(), db, source, link.DirectionHost); err != nil {
			return err
		}
		if err := SaveResourceCatalogSource(t.Context(), db, entry, stale); err != nil {
			return err
		}
		if err := SetLinkDirection(t.Context(), db, source, link.DirectionController); err != nil {
			return err
		}
		values, err := ReadResourceCatalogSources(t.Context(), db, entry)
		if len(values) != 0 {
			return fmt.Errorf("removing the outbound direction retained private directory entries")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func TestResourceCatalogReplacesOnlyCompleteSuccessfulSources(t *testing.T) {
	t.Parallel()
	_, entry, storage := schemaFixture(t)
	source, other := protocol.NodeID{91}, protocol.NodeID{92}
	project := protocol.Project{ID: protocol.NewProjectID(), Name: "Retained project"}
	initial := protocol.ResourceSource{Node: source, Route: protocol.RouteKey{Entry: entry, Target: source}, Projects: []protocol.Project{project}}
	if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error { return PairLinkPeer(t.Context(), db, source, link.DirectionHost) }); err != nil {
		t.Fatal(err)
	}
	write := func(value protocol.ResourceSource) error {
		return storage.Node.Write(t.Context(), func(db *sql.Conn) error {
			return SaveResourceCatalogSource(t.Context(), db, entry, value)
		})
	}
	if err := write(initial); err != nil {
		t.Fatal(err)
	}
	for _, code := range []protocol.ErrorCode{protocol.ErrorCodeUnavailable, protocol.ErrorCodeCancelled, protocol.ErrorCodeOutcomeUnknown} {
		incomplete := initial
		incomplete.Projects, incomplete.Fault = nil, &protocol.Fault{Code: code, Message: "interrupted snapshot"}
		if err := write(incomplete); err == nil {
			t.Fatalf("incomplete source %s replaced the complete directory", code)
		}
	}
	if err := storage.Node.Read(t.Context(), func(db *sql.Conn) error {
		values, err := ReadResourceCatalogSources(t.Context(), db, entry)
		if err == nil && (len(values) != 1 || len(values[0].Projects) != 1 || values[0].Projects[0].ID != project.ID) {
			return fmt.Errorf("failed snapshot deleted the old directory: %+v", values)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	initial.Projects = nil // A complete successful snapshot confirms deletion.
	if err := write(initial); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(t.Context(), func(db *sql.Conn) error {
		values, err := ReadResourceCatalogSources(t.Context(), db, entry)
		if err != nil {
			return err
		}
		if len(values) != 1 || len(values[0].Projects) != 0 {
			return fmt.Errorf("complete deletion did not replace the directory: %+v", values)
		}
		removed, err := PruneResourceCatalogSources(t.Context(), db, entry, []protocol.NodeID{other})
		if err != nil {
			return err
		}
		if len(removed) != 1 || removed[0] != source {
			return fmt.Errorf("removed connection retained a disk projection: %v", removed)
		}
		values, err = ReadResourceCatalogSources(t.Context(), db, entry)
		if len(values) != 0 {
			return fmt.Errorf("pruned source remains in the directory")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}
