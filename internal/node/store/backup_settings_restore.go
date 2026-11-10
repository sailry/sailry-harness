package store

import (
	"context"
	"database/sql"
	"errors"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// PrepareRestored validates an unpacked profile in private staging, upgrades
// its Node database to the current schema and rewrites the paths that pointed
// into the original profile. It returns the staging-relative files of the
// pre-upgrade database copy, which the restored profile retains.
func PrepareRestored(ctx context.Context, staging string, node protocol.NodeID, original, target string) ([]string, error) {
	storage := filepath.Join(staging, "storage")
	handle, err := OpenHandle(ctx, filepath.Join(storage, "node.sqlite"))
	if err != nil {
		return nil, err
	}
	err = prepareRestored(ctx, handle, node, original, target)
	if err == nil {
		// The checkpoint leaves the family complete in its main file.
		var closed *Closed
		if closed, err = handle.Pause(ctx); err == nil {
			err = closed.Close()
		}
	}
	if err = errors.Join(err, handle.Close()); err != nil {
		return nil, err
	}
	if err := checkRestoredKnowledge(ctx, filepath.Join(storage, "knowledge.sqlite")); err != nil {
		return nil, err
	}
	var retained []string
	err = filepath.WalkDir(filepath.Join(storage, "backups"), func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.Type().IsRegular() {
			relative, err := filepath.Rel(staging, path)
			if err != nil {
				return err
			}
			retained = append(retained, filepath.ToSlash(relative))
		}
		return nil
	})
	if err != nil && !errors.Is(err, fs.ErrNotExist) {
		return nil, err
	}
	return retained, nil
}

func prepareRestored(ctx context.Context, handle *Handle, node protocol.NodeID, original, target string) error {
	definition, err := prepareSchema(ctx, handle, node)
	if err != nil {
		return err
	}
	if definition.state == schemaFresh || definition.state == schemaRebuild {
		return invalid("restored database is not a current Node database")
	}
	if definition.state == schemaUpdate {
		if err := rehearseSchemaUpdate(ctx, handle, definition, node); err != nil {
			return err
		}
	}
	return handle.Write(ctx, func(connection *sql.Conn) error {
		// A current database is validated as strictly as an upgraded one.
		if definition.state == schemaCurrent {
			if err := validateStored(ctx, connection); err != nil {
				return err
			}
		}
		if err := definition.initialize(ctx, connection, node); err != nil {
			return err
		}
		return relocateRestored(ctx, connection, original, target)
	})
}

func checkRestoredKnowledge(ctx context.Context, path string) error {
	if _, err := os.Lstat(path); errors.Is(err, fs.ErrNotExist) {
		return nil
	} else if err != nil {
		return err
	}
	handle, err := OpenHandle(ctx, path)
	if err != nil {
		return err
	}
	err = handle.Read(ctx, func(connection *sql.Conn) error {
		var integrity string
		if err := connection.QueryRowContext(ctx, "PRAGMA integrity_check").Scan(&integrity); err != nil {
			return err
		}
		if integrity != "ok" {
			return invalid("restored knowledge database is corrupt")
		}
		return nil
	})
	if err == nil {
		var closed *Closed
		if closed, err = handle.Pause(ctx); err == nil {
			err = closed.Close()
		}
	}
	return errors.Join(err, handle.Close())
}

func relocateRestored(ctx context.Context, db *sql.Conn, original, target string) error {
	for _, table := range []string{"projects", "worktrees"} {
		rows, err := db.QueryContext(ctx, "SELECT id,path FROM "+table)
		if err != nil {
			return err
		}
		paths := map[string]string{}
		for rows.Next() {
			var id, path string
			if err = rows.Scan(&id, &path); err != nil {
				break
			}
			paths[id] = path
		}
		if err := errors.Join(err, rows.Err(), rows.Close()); err != nil {
			return err
		}
		for id, path := range paths {
			if moved, ok := relocated(path, original, target); ok {
				if _, err := db.ExecContext(ctx, "UPDATE "+table+" SET path=?1 WHERE id=?2", moved, id); err != nil {
					return err
				}
			}
		}
	}
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return err
	}
	for _, stored := range entries {
		source := &stored.Collection.Definition.Source
		var path string
		switch source.Kind {
		case "file":
			value, _ := dispatchValue[protocol.KnowledgeSourceFilePayload](source.Data)
			path = value.Path
		case "directory":
			value, _ := dispatchValue[protocol.KnowledgeSourceDirectoryPayload](source.Data)
			path = value.Path
		default:
			continue
		}
		moved, ok := relocated(path, original, target)
		if !ok {
			continue
		}
		if source.Kind == "file" {
			source.Data = protocol.KnowledgeSourceFilePayload{Path: moved}
		} else {
			source.Data = protocol.KnowledgeSourceDirectoryPayload{Path: moved}
		}
		body, err := encode(stored)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE knowledge_collections SET body=?1 WHERE id=?2", body, stored.Collection.ID.String()); err != nil {
			return err
		}
	}
	return nil
}

func relocated(value, original, target string) (string, bool) {
	relative, err := filepath.Rel(original, value)
	if err != nil || relative != "." && !filepath.IsLocal(relative) {
		return "", false
	}
	return filepath.Join(target, relative), true
}
