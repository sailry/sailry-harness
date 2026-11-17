package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Resource catalogs are projections of the current user's authenticated direct
// routes. They deliberately cannot store a full Snapshot, credentials, session
// history, file content or knowledge indexes.
func ReadResourceCatalogSources(ctx context.Context, db *sql.Conn, entry protocol.NodeID) ([]protocol.ResourceSource, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	rows, err := db.QueryContext(ctx, "SELECT body FROM resource_catalog_sources WHERE user=?1 ORDER BY source", user.ID.String())
	if err != nil {
		return nil, err
	}
	values := []protocol.ResourceSource{}
	for rows.Next() {
		var body []byte
		var value protocol.ResourceSource
		if err := rows.Scan(&body); err != nil {
			rows.Close()
			return nil, err
		}
		if err := json.Unmarshal(body, &value); err != nil {
			rows.Close()
			return nil, err
		}
		if value.Route.Entry != entry || value.Route.Target != value.Node || value.Node == entry {
			rows.Close()
			return nil, fault(protocol.ErrorCodeInvalidRequest, "cached resource route belongs to another entry")
		}
		values = append(values, value)
	}
	return values, closeRows(rows)
}

// SaveResourceCatalogSource publishes a complete directory version. Callers
// retain the previous version while snapshot pages are incomplete or a source
// is temporarily unavailable; absence means deletion only in a complete read.
func SaveResourceCatalogSource(ctx context.Context, db *sql.Conn, entry protocol.NodeID, source protocol.ResourceSource) error {
	if source.Node == entry || source.Route != (protocol.RouteKey{Entry: entry, Target: source.Node}) || source.Fault != nil {
		return fault(protocol.ErrorCodeInvalidRequest, "only a successful direct resource projection can be cached")
	}
	if slices.ContainsFunc(source.Knowledge, func(collection protocol.KnowledgeCollection) bool { return collection.Node != source.Node }) {
		return fault(protocol.ErrorCodeInvalidRequest, "knowledge directory belongs to another source")
	}
	var accessible bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM link_peers WHERE identity=?1 AND direction IN ('host','both'))", source.Node[:]).Scan(&accessible); err != nil {
		return err
	}
	if !accessible {
		return fault(protocol.ErrorCodePermissionDenied, "resource source connection was removed")
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	// Only the directory identity, location, visibility and confirmed version
	// survive restart. Indexing recipes and diagnostic details are not a copy
	// of the remote collection's authoritative configuration or index.
	source.Knowledge = slices.Clone(source.Knowledge)
	for index := range source.Knowledge {
		collection := &source.Knowledge[index]
		collection.Definition = protocol.KnowledgeDefinition{
			Name: collection.Definition.Name, Visibility: collection.Definition.Visibility, Source: collection.Definition.Source,
		}
		status := collection.Progress.Status
		collection.Progress = protocol.DefaultKnowledgeProgress()
		collection.Progress.Status = status
		collection.Rebuild = 0
	}
	body, err := encode(source)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO resource_catalog_sources(user,source,body) VALUES(?1,?2,?3)
ON CONFLICT(user,source) DO UPDATE SET body=excluded.body`, user.ID.String(), source.Node[:], body)
	return err
}

func RemoveResourceCatalogSource(ctx context.Context, db *sql.Conn, source protocol.NodeID) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "DELETE FROM resource_catalog_sources WHERE user=?1 AND source=?2", user.ID.String(), source[:])
	return err
}

// PruneResourceCatalogSources follows explicit relationship removal, including
// removals made while this Node was stopped. A failed peer listing must not call
// this with an empty list.
func PruneResourceCatalogSources(ctx context.Context, db *sql.Conn, entry protocol.NodeID, allowed []protocol.NodeID) ([]protocol.NodeID, error) {
	values, err := ReadResourceCatalogSources(ctx, db, entry)
	if err != nil {
		return nil, err
	}
	removed := []protocol.NodeID{}
	for _, value := range values {
		if slices.Contains(allowed, value.Node) {
			continue
		}
		if err := RemoveResourceCatalogSource(ctx, db, value.Node); err != nil {
			return nil, err
		}
		removed = append(removed, value.Node)
	}
	return removed, nil
}
