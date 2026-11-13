package store

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const applicationID = 0x5341494c

type Store struct {
	Node      *Handle
	Knowledge *Handle
	NodeID    protocol.NodeID
	Profile   string
}

func (storage *Store) Close() error {
	var knowledge, node error
	if storage.Knowledge != nil {
		knowledge = storage.Knowledge.Close()
	}
	if storage.Node != nil {
		node = storage.Node.Close()
	}
	return errors.Join(knowledge, node)
}

// CloseIdle closes each database that no read or write is using.
func (storage *Store) CloseIdle() error {
	var knowledge, node error
	if storage.Knowledge != nil {
		knowledge = storage.Knowledge.closeIdle()
	}
	if storage.Node != nil {
		node = storage.Node.closeIdle()
	}
	return errors.Join(knowledge, node)
}

// rustDatabases are the database files the Rust Node kept in storage. Each
// name also covers its side files (name-wal, name-log and the like).
var rustDatabases = [...]string{
	"node.turso", "knowledge.turso", "node.db", "knowledge.db", "node.sqlite3", "knowledge.sqlite3",
}

// RustArchive receives the Rust databases and their collected file resources.
// A later set of them goes to a numbered sibling ("rust-archive-2", ...).
const RustArchive = "rust-archive"

// archiveRustDatabases moves the Rust Node's databases out of storage,
// unopened and unchanged; nothing is imported, replaced or deleted. Packages
// and attachments are copied before any database moves: their only remaining
// references may be in those databases, outside the current store's collectors.
// Live resources stay in place because a current Go database can reference them
// too. A later set goes to an archive that holds none of its target names.
func archiveRustDatabases(ctx context.Context, storage string) error {
	entries, err := os.ReadDir(storage)
	if err != nil {
		return err
	}
	var names []string
	for _, entry := range entries {
		name := entry.Name()
		if entry.Type().IsRegular() && slices.ContainsFunc(rustDatabases[:], func(database string) bool {
			return name == database || strings.HasPrefix(name, database+"-")
		}) {
			names = append(names, name)
		}
	}
	if len(names) == 0 {
		return nil
	}
	profile := filepath.Dir(storage)
	var resources []string
	for _, name := range []string{"plugins/packages", "attachments"} {
		if _, err := os.Lstat(filepath.Join(profile, filepath.FromSlash(name))); err == nil {
			resources = append(resources, name)
		} else if !errors.Is(err, os.ErrNotExist) {
			return err
		}
	}
	archive := filepath.Join(storage, RustArchive)
	for number := 2; ; number++ {
		held := false
		for _, name := range append(names, resources...) {
			if _, err := os.Lstat(filepath.Join(archive, name)); err == nil {
				held = true
				break
			} else if !errors.Is(err, os.ErrNotExist) {
				return err
			}
		}
		if !held {
			break
		}
		archive = filepath.Join(storage, fmt.Sprintf("%s-%d", RustArchive, number))
	}
	if err := core.PrivateDirectory(archive); err != nil {
		return err
	}
	if len(resources) != 0 {
		profile, err = filepath.Abs(profile)
		if err == nil {
			profile, err = filepath.EvalSymlinks(profile)
		}
		if err != nil {
			return err
		}
		files, err := core.OpenFiles(profile, core.ManualFiles)
		if err != nil {
			return err
		}
		defer files.Close()
		for _, name := range resources {
			if err := core.PrivateDirectory(filepath.Dir(filepath.Join(archive, filepath.FromSlash(name)))); err != nil {
				return err
			}
			target := filepath.ToSlash(filepath.Join("storage", filepath.Base(archive), filepath.FromSlash(name)))
			if err := files.CopyEntry(ctx, name, target); err != nil {
				return fmt.Errorf("archive Rust resource %q: %w", name, err)
			}
		}
	}
	for _, name := range names {
		if err := os.Rename(filepath.Join(storage, name), filepath.Join(archive, name)); err != nil {
			return fmt.Errorf("archive Rust database %q: %w", name, err)
		}
	}
	return nil
}

// Open is called only by the owner of the profile's advisory lock.
func Open(ctx context.Context, profile string, node protocol.NodeID) (_ *Store, failure error) {
	storage := filepath.Join(profile, "storage")
	if err := core.PrivateDirectory(storage); err != nil {
		return nil, err
	}
	if err := archiveRustDatabases(ctx, storage); err != nil {
		return nil, err
	}
	path := filepath.Join(storage, "node.sqlite")
	handle, err := OpenHandle(ctx, path)
	if err != nil {
		return nil, err
	}
	defer func() {
		if failure != nil && handle != nil {
			_ = handle.Close()
		}
	}()
	definition, err := prepareSchema(ctx, handle, node)
	if err != nil {
		return nil, err
	}
	if definition.state == schemaUpdate {
		if err := rehearseSchemaUpdate(ctx, handle, definition, node); err != nil {
			return nil, err
		}
	}
	if err := handle.Write(ctx, func(connection *sql.Conn) error {
		return definition.initialize(ctx, connection, node)
	}); err != nil {
		return nil, err
	}
	if definition.ledger {
		// Fold the log into the database file and truncate it, so neither
		// keeps a page image that precedes the replacement.
		closed, err := handle.Pause(ctx)
		if err != nil {
			return nil, err
		}
		if err := closed.Reopen(ctx); err != nil {
			return nil, err
		}
	}
	knowledge, err := OpenHandle(ctx, filepath.Join(storage, "knowledge.sqlite"))
	if err != nil {
		return nil, err
	}
	if err := knowledge.Write(ctx, func(connection *sql.Conn) error {
		if _, err := connection.ExecContext(ctx, sqliteSchema(KnowledgeSchema)); err != nil {
			return err
		}
		var version int64
		if err := connection.QueryRowContext(ctx, "SELECT version FROM metadata WHERE singleton=1").Scan(&version); err != nil {
			return err
		}
		if version != 1 {
			return errors.New("knowledge index: unexpected derived index format")
		}
		return nil
	}); err != nil {
		_ = knowledge.Close()
		return nil, err
	}
	return &Store{Node: handle, Knowledge: knowledge, NodeID: node, Profile: profile}, nil
}

type schemaState uint8

const (
	schemaFresh schemaState = iota
	schemaCurrent
	schemaUpdate
)

type currentSchema struct {
	state    schemaState
	expected schemaObjects
	plan     schemaPlan
	path     string
	// ledger reports admissions of secret-bearing commands stored in full.
	ledger bool
}

func prepareSchema(ctx context.Context, handle *Handle, node protocol.NodeID) (currentSchema, error) {
	definition := currentSchema{path: handle.Path()}
	err := handle.Read(ctx, func(connection *sql.Conn) error {
		fresh, err := schemaOwnership(ctx, connection, definition.path)
		if err != nil {
			return err
		}
		if !fresh {
			if err := verifyNodeIdentity(ctx, connection, definition.path, node); err != nil {
				return err
			}
			if err := verifyGeneration(ctx, connection, definition.path); err != nil {
				return err
			}
		}
		var pluginIndexes []string
		if !fresh {
			var declared uint64
			if err := connection.QueryRowContext(ctx, "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('plugins','plugin_values')").Scan(&declared); err != nil {
				return err
			}
			if declared == 2 {
				pluginIndexes, err = PluginSearchDefinitions(ctx, connection)
				if err != nil {
					return err
				}
			}
		}
		definition.expected, err = referenceSchema(ctx, filepath.Dir(handle.Path()), pluginIndexes...)
		if err != nil {
			return err
		}
		if fresh {
			definition.state = schemaFresh
			return nil
		}
		definition.plan, err = prepareSchemaPlan(ctx, connection, definition.path, definition.expected)
		if err != nil {
			return err
		}
		definition.state = schemaUpdate
		if !definition.plan.created["requests"] {
			if definition.ledger, err = ledgerSecretsPending(ctx, connection); err != nil {
				return err
			}
		}
		if len(definition.plan.statements) != 0 {
			return nil
		}
		if err := verifySchemaObjects(ctx, connection, definition.path, definition.expected); err != nil {
			return err
		}
		pending, err := upgradesNeeded(ctx, connection)
		if err != nil {
			return err
		}
		if !pending && !definition.ledger {
			definition.state = schemaCurrent
		}
		return nil
	})
	return definition, err
}

func (definition currentSchema) initialize(ctx context.Context, connection *sql.Conn, node protocol.NodeID) error {
	switch definition.state {
	case schemaFresh:
		if _, err := connection.ExecContext(ctx, sqliteSchema(NodeSchema)); err != nil {
			return err
		}
	case schemaUpdate:
		if err := definition.plan.apply(ctx, connection); err != nil {
			return err
		}
		if err := applyUpgrades(ctx, connection); err != nil {
			return err
		}
	}
	if definition.state == schemaUpdate && definition.plan.added[schemaObject{"database_profiles", "password_target"}] {
		if err := reconcileDatabaseTargets(ctx, connection); err != nil {
			return err
		}
	}
	if definition.state != schemaCurrent {
		if err := initializeDefaults(ctx, connection); err != nil {
			return err
		}
		if definition.state == schemaFresh || definition.plan.created["roles"] {
			if err := initializeRoles(ctx, connection); err != nil {
				return err
			}
		}
		if definition.state == schemaFresh || definition.plan.created["local_model_settings"] {
			if err := initializeLocalModels(ctx, connection); err != nil {
				return err
			}
		}
		if err := verifySchemaObjects(ctx, connection, definition.path, definition.expected); err != nil {
			return err
		}
	}
	if _, err := connection.ExecContext(ctx, fmt.Sprintf("PRAGMA application_id=%d; PRAGMA user_version=1", applicationID)); err != nil {
		return err
	}
	var stored []byte
	err := connection.QueryRowContext(ctx, "SELECT identity FROM node WHERE singleton=1").Scan(&stored)
	switch {
	case errors.Is(err, sql.ErrNoRows):
		if _, err := connection.ExecContext(ctx, "INSERT INTO node(singleton,identity,tool_defaults,tool_metadata_names) VALUES(1,?1,1,1)", node[:]); err != nil {
			return err
		}
	case err != nil:
		return err
	case !bytes.Equal(stored, node[:]):
		return errors.New("Node identity does not match its database")
	}
	validate := validateStored
	if definition.state == schemaCurrent {
		validate = validateStartup
	}
	if err := validate(ctx, connection); err != nil {
		return err
	}
	// Every connection enforces foreign keys, so a database this build left
	// current holds no orphan and is not scanned again at each start. One
	// that was just created, updated or restored is audited.
	if definition.state == schemaCurrent {
		return nil
	}
	return auditReferences(ctx, connection)
}

func rehearseSchemaUpdate(ctx context.Context, handle *Handle, definition currentSchema, node protocol.NodeID) error {
	closed, err := handle.Pause(ctx)
	if err != nil {
		return err
	}
	defer closed.Close()
	preview, err := previewDatabase(handle.Path())
	if err != nil {
		return err
	}
	defer preview.close()
	rehearsal, err := OpenHandle(ctx, preview.path)
	if err != nil {
		return err
	}
	rehearsalError := rehearsal.Write(ctx, func(connection *sql.Conn) error {
		return definition.initialize(ctx, connection, node)
	})
	if err := errors.Join(rehearsalError, rehearsal.Close()); err != nil {
		return err
	}
	backup, err := saveBeforeUpgrade(handle.Path())
	if err != nil {
		return err
	}
	if definition.ledger {
		// The backup would otherwise keep the secrets this update removes.
		if err := digestLedgerCopy(ctx, backup); err != nil {
			return err
		}
	}
	return closed.Reopen(ctx)
}

func schemaOwnership(ctx context.Context, connection *sql.Conn, path string) (bool, error) {
	var application, version int64
	if err := connection.QueryRowContext(ctx, "PRAGMA application_id").Scan(&application); err != nil {
		return false, err
	}
	if err := connection.QueryRowContext(ctx, "PRAGMA user_version").Scan(&version); err != nil {
		return false, err
	}
	if application == 0 && version == 0 {
		objects, err := readSchemaObjects(ctx, connection)
		if err != nil {
			return false, err
		}
		if len(objects) == 0 {
			return true, nil
		}
	}
	if application != applicationID {
		return false, incompatibleDatabase(path, fmt.Sprintf("application_id is %#x, expected %#x", application, applicationID))
	}
	if version != 1 {
		return false, incompatibleDatabase(path, fmt.Sprintf("user_version is %d, expected 1", version))
	}
	return false, nil
}

func verifyNodeIdentity(ctx context.Context, connection *sql.Conn, path string, node protocol.NodeID) error {
	var stored []byte
	if err := connection.QueryRowContext(ctx, "SELECT identity FROM node WHERE singleton=1").Scan(&stored); err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if !bytes.Equal(stored, node[:]) {
		return incompatibleDatabase(path, "Node identity is missing or does not match its database")
	}
	return nil
}

// verifyGeneration accepts only generation 1. Storage in any other state is
// refused and left untouched; nothing here removes or resets a database.
func verifyGeneration(ctx context.Context, connection *sql.Conn, path string) error {
	var declared, current bool
	if err := connection.QueryRowContext(ctx, "SELECT count(*) FROM pragma_table_info('node') WHERE name='storage_generation'").Scan(&declared); err != nil {
		return err
	}
	if !declared {
		return incompatibleDatabase(path, "node.storage_generation is missing")
	}
	if err := connection.QueryRowContext(ctx, "SELECT CASE WHEN typeof(storage_generation)='integer' AND storage_generation=1 THEN 1 ELSE 0 END FROM node WHERE singleton=1").Scan(&current); err != nil {
		return err
	}
	if !current {
		return incompatibleDatabase(path, "node.storage_generation is invalid or unsupported")
	}
	return nil
}

func referenceSchema(ctx context.Context, parent string, additional ...string) (schemaObjects, error) {
	directory := filepath.Join(parent, ".schema-reference-"+uuid.NewString())
	if err := core.PrivateDirectory(directory); err != nil {
		return nil, err
	}
	defer os.RemoveAll(directory)
	handle, err := OpenHandle(ctx, filepath.Join(directory, "reference.sqlite"))
	if err != nil {
		return nil, err
	}
	defer handle.Close()
	if err := handle.Write(ctx, func(connection *sql.Conn) error {
		if _, err := connection.ExecContext(ctx, sqliteSchema(NodeSchema)); err != nil {
			return err
		}
		for _, definition := range additional {
			if _, err := connection.ExecContext(ctx, definition); err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		return nil, err
	}
	var objects schemaObjects
	err = handle.Read(ctx, func(connection *sql.Conn) error {
		var err error
		objects, err = readSchemaObjects(ctx, connection)
		return err
	})
	return objects, err
}

func readSchemaObjects(ctx context.Context, connection *sql.Conn) (schemaObjects, error) {
	rows, err := connection.QueryContext(ctx, "SELECT name,type,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND sql IS NOT NULL")
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	objects := make(schemaObjects)
	for rows.Next() {
		var name, kind, declaration string
		if err := rows.Scan(&name, &kind, &declaration); err != nil {
			return nil, err
		}
		objects[schemaObject{kind, name}] = declaration
	}
	return objects, rows.Err()
}

func sortedObjects(objects schemaObjects) []schemaObject {
	identities := make([]schemaObject, 0, len(objects))
	for identity := range objects {
		identities = append(identities, identity)
	}
	slices.SortFunc(identities, func(left, right schemaObject) int {
		if comparison := strings.Compare(left.kind, right.kind); comparison != 0 {
			return comparison
		}
		return strings.Compare(left.name, right.name)
	})
	return identities
}

func removeRetainedTables(actual schemaObjects, path string) error {
	retained := map[string]string{
		"code_settings": "CREATE TABLE code_settings (singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL)",
		"code_startups": "CREATE TABLE code_startups (worktree TEXT NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,root TEXT NOT NULL,server TEXT NOT NULL,body BLOB NOT NULL,PRIMARY KEY(worktree,root,server))",
	}
	for _, name := range sortedKeys(retained) {
		identity := schemaObject{"table", name}
		if stored, exists := actual[identity]; exists {
			if normalizeSQL(stored) != normalizeSQL(retained[name]) {
				return incompatibleDatabase(path, fmt.Sprintf("retained table %q does not match its definition", name))
			}
			delete(actual, identity)
		}
	}
	return nil
}

func verifySchemaObjects(ctx context.Context, connection *sql.Conn, path string, expected schemaObjects) error {
	actual, err := readSchemaObjects(ctx, connection)
	if err != nil {
		return err
	}
	if err := removeRetainedTables(actual, path); err != nil {
		return err
	}
	for _, identity := range sortedObjects(expected) {
		storedSQL, exists := actual[identity]
		if !exists {
			return incompatibleDatabase(path, fmt.Sprintf("schema object %q is missing", identity.name))
		}
		if normalizeSQL(storedSQL) == normalizeSQL(expected[identity]) {
			continue
		}
		if identity.kind != "table" {
			return incompatibleDatabase(path, fmt.Sprintf("schema object %q does not match the current definition", identity.name))
		}
		wanted, err := parseTable(expected[identity])
		if err != nil {
			return err
		}
		stored, err := parseTable(storedSQL)
		if err != nil {
			return err
		}
		for _, field := range sortedKeys(wanted.columns) {
			existing, exists := stored.columns[field]
			if !exists {
				return incompatibleDatabase(path, fmt.Sprintf("column %s.%s is missing", identity.name, field))
			}
			if schemaColumn(identity.name, field, existing) != schemaColumn(identity.name, field, wanted.columns[field]) {
				return incompatibleDatabase(path, fmt.Sprintf("column %s.%s does not match the current definition", identity.name, field))
			}
		}
		for _, field := range sortedKeys(stored.columns) {
			if _, exists := wanted.columns[field]; !exists {
				return incompatibleDatabase(path, fmt.Sprintf("column %s.%s is not part of the current schema", identity.name, field))
			}
		}
		if !sameTableShape(wanted, stored) {
			return incompatibleDatabase(path, fmt.Sprintf("table %q does not match the current constraints", identity.name))
		}
	}
	for _, identity := range sortedObjects(actual) {
		if _, exists := expected[identity]; !exists {
			return incompatibleDatabase(path, fmt.Sprintf("schema object %q (%s) is not part of the current schema", identity.name, identity.kind))
		}
	}
	return nil
}

// auditReferences refuses storage in which a row names a missing parent.
func auditReferences(ctx context.Context, connection *sql.Conn) error {
	tables, err := schemaTableNames(NodeSchema)
	if err != nil {
		return err
	}
	for _, table := range tables {
		rows, err := connection.QueryContext(ctx, "PRAGMA foreign_key_check("+quoteIdentifier(table)+")")
		if err != nil {
			return err
		}
		invalid := rows.Next()
		if err := errors.Join(rows.Err(), rows.Close()); err != nil {
			return err
		}
		if invalid {
			return fmt.Errorf("invalid stored reference in %s; existing data was not changed", table)
		}
	}
	return nil
}
