package node

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const legacyProfileMarker = ".legacy-profile-migration"

type legacyProfileState struct {
	Version  int               `json:"version"`
	Legacy   string            `json:"legacy"`
	Target   string            `json:"target"`
	Attempt  string            `json:"attempt"`
	Node     protocol.NodeID   `json:"node"`
	Original map[string]string `json:"original"`
	Prepared map[string]string `json:"prepared"`
}

type legacyProfileMigration struct {
	legacy, declared, target, directory string
	state                               legacyProfileState
}

// MigrateLegacyProfile adopts a stopped Node profile beside target. Existing
// targets are left alone. Interrupted migrations retain their journal, original
// database families and complete unpublished profile for a later invocation.
// No alias is installed at the old name, and user trees are not walked.
func MigrateLegacyProfile(ctx context.Context, legacy, target string) error {
	if legacy == "" {
		return nil
	}
	legacy, err := identity.ResolveProfile(legacy)
	if err != nil {
		return err
	}
	return migrateLegacyProfile(ctx, legacy, target, nil)
}

func migrateLegacyProfile(ctx context.Context, legacy, target string, checkpoint func(string) error) (failure error) {
	if !filepath.IsAbs(target) {
		return &core.InvalidProfileError{Path: target, Reason: "legacy migration requires an absolute target"}
	}
	legacy, target = filepath.Clean(legacy), filepath.Clean(target)
	if legacy == target {
		return nil
	}
	if exists, err := legacyProfileExists(legacy); err != nil {
		return err
	} else if !exists {
		journal := filepath.Join(filepath.Dir(target), "."+filepath.Base(target)+".legacy-profile", "state.json")
		if exists, err := legacyProfileExists(journal); err != nil || !exists {
			return err
		}
	}
	// Resolve only the two parents, never the profile trees or their links.
	parent, err := filepath.EvalSymlinks(filepath.Dir(legacy))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	targetParent, err := filepath.EvalSymlinks(filepath.Dir(target))
	if err != nil {
		return err
	}
	left, err := os.Stat(parent)
	if err != nil {
		return err
	}
	right, err := os.Stat(targetParent)
	if err != nil {
		return err
	}
	if !os.SameFile(left, right) {
		return &core.InvalidProfileError{Path: legacy, Reason: "legacy and target profiles must have the same parent"}
	}
	migration := &legacyProfileMigration{
		legacy: filepath.Join(parent, filepath.Base(legacy)), declared: legacy, target: filepath.Join(parent, filepath.Base(target)),
		directory: filepath.Join(parent, "."+filepath.Base(target)+".legacy-profile"),
	}
	if migration.legacy == migration.target || migration.legacy == migration.directory {
		return &core.InvalidProfileError{Path: legacy, Reason: "legacy migration paths overlap"}
	}
	targetExists, err := legacyProfileExists(migration.target)
	if err != nil {
		return err
	}
	state, err := migration.readState()
	completed := err == nil && migration.matchesFile(filepath.Join(migration.directory, "done"), state.Attempt)
	if targetExists && !completed && (err != nil || !migration.matchesMarker(migration.target, state.Attempt)) {
		// In particular, do not open, chmod, lock or initialize an existing target.
		if err == nil && migration.matchesMarker(filepath.Join(migration.directory, "profile"), state.Attempt) {
			return fmt.Errorf("legacy migration target %q already exists; the unpublished profile remains at %q", migration.target, filepath.Join(migration.directory, "profile"))
		}
		return nil
	}
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if !targetExists && errors.Is(err, os.ErrNotExist) {
		exists, err := legacyProfileExists(migration.legacy)
		if err != nil || !exists {
			return err
		}
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	// The normal profile lock also serializes migration/recovery on Windows.
	ownership, err := core.AcquireProfile(migration.directory)
	if err != nil {
		return err
	}
	defer func() { failure = errors.Join(failure, ownership.Close()) }()
	state, err = migration.readState()
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if err == nil {
		migration.state = state
	} else {
		source, err := core.AcquireProfile(migration.legacy)
		if err != nil {
			return err
		}
		defer func() { failure = errors.Join(failure, source.Close()) }()
		if err := migration.prepare(ctx); err != nil {
			return err
		}
		if checkpoint != nil {
			if err := checkpoint("prepared"); err != nil {
				return err
			}
		}
		return migration.resume(ctx, source, checkpoint)
	}
	return migration.resume(ctx, nil, checkpoint)
}

func (migration *legacyProfileMigration) readState() (legacyProfileState, error) {
	var state legacyProfileState
	if err := legacyProfileReadJSON(filepath.Join(migration.directory, "state.json"), &state); err != nil {
		return state, err
	}
	if state.Version != 1 || state.Legacy != migration.legacy || state.Target != migration.target ||
		!strings.HasPrefix(state.Attempt, "attempt-") || filepath.Base(state.Attempt) != state.Attempt ||
		len(state.Original) == 0 || len(state.Prepared) == 0 {
		return state, fmt.Errorf("invalid legacy migration journal at %q; retained files were not changed", migration.directory)
	}
	for _, files := range []map[string]string{state.Original, state.Prepared} {
		for name, sum := range files {
			if !legacyProfileDatabaseName(name) || len(sum) != sha256.Size*2 {
				return state, fmt.Errorf("invalid legacy migration database manifest at %q", migration.directory)
			}
		}
	}
	return state, nil
}

func (migration *legacyProfileMigration) prepare(ctx context.Context) (failure error) {
	if _, err := legacyProfileRegular(filepath.Join(migration.legacy, "link.key")); err != nil {
		return err
	}
	if err := checkLegacyProfileRootLinks(migration.legacy, migration.declared); err != nil {
		return err
	}
	if err := checkLegacyProfileWorktrees(migration.legacy); err != nil {
		return err
	}
	if err := checkLegacyProfilePython(migration.legacy); err != nil {
		return err
	}
	node, err := link.ReadIdentity(migration.legacy)
	if err != nil {
		return err
	}
	if exists, err := legacyProfileExists(migration.backup()); err != nil || exists {
		return errors.Join(err, fmt.Errorf("original legacy database backup already exists at %q", migration.backup()))
	}
	base := filepath.Join(migration.directory, "preparation")
	if exists, err := legacyProfileExists(base); err != nil {
		return err
	} else if exists {
		if !migration.ownsPreparation("") {
			return fmt.Errorf("unrecognized legacy preparation directory at %q", base)
		}
		if err := os.RemoveAll(base); err != nil {
			return err
		}
	}
	if err := os.Mkdir(base, 0o700); err != nil {
		return err
	}
	defer func() {
		if failure != nil {
			// Only copied databases live here. Once the durable journal exists,
			// recovery owns these copies instead of this failed preflight.
			if state, err := migration.readState(); err != nil || state.Attempt != migration.state.Attempt {
				failure = errors.Join(failure, os.RemoveAll(base))
			}
		}
	}()
	if err := core.PrivateDirectory(base); err != nil {
		return err
	}
	attempt := "attempt-" + uuid.NewString()
	migration.state = legacyProfileState{Version: 1, Legacy: migration.legacy, Target: migration.target, Attempt: attempt, Node: node.ID()}
	owner, err := json.Marshal(migration.state)
	if err != nil {
		return err
	}
	if err := writeLegacyProfileFile(filepath.Join(base, "owner.json"), owner); err != nil {
		return err
	}
	original, prepared := filepath.Join(base, "original"), filepath.Join(base, "prepared")
	for _, directory := range []string{original, prepared, filepath.Join(original, "storage"), filepath.Join(prepared, "storage")} {
		if err := core.PrivateDirectory(directory); err != nil {
			return err
		}
	}
	// No engine has opened the source. Exclusive Node ownership keeps the whole
	// closed native family stable, including an uncheckpointed crash log.
	originalFiles, err := copyLegacyProfileDatabases(ctx, filepath.Join(migration.legacy, "storage"), filepath.Join(original, "storage"))
	if err != nil {
		return err
	}
	if _, err := copyLegacyProfileFile(ctx, filepath.Join(migration.legacy, "link.key"), filepath.Join(original, "link.key")); err != nil {
		return err
	}
	if _, err := copyLegacyProfileDatabases(ctx, filepath.Join(original, "storage"), filepath.Join(prepared, "storage")); err != nil {
		return err
	}
	if _, err := copyLegacyProfileFile(ctx, filepath.Join(original, "link.key"), filepath.Join(prepared, "link.key")); err != nil {
		return err
	}
	// PrepareRestored is the store's real integrity, ownership, identity and
	// schema qualification. Its writes and upgrade cleanup affect only our copy.
	if _, err := store.PrepareRestored(ctx, prepared, node.ID(), migration.legacy, migration.legacy); err != nil {
		return fmt.Errorf("legacy profile failed database qualification: %w", err)
	}
	handle, err := store.OpenHandle(ctx, filepath.Join(prepared, "storage", "node.turso"))
	if err != nil {
		return err
	}
	err = handle.Read(ctx, func(db *sql.Conn) error {
		return checkLegacyProfileRegisteredGit(ctx, db, migration.legacy, migration.declared)
	})
	if err == nil {
		err = handle.Write(ctx, func(db *sql.Conn) error {
			if err := store.RelocateProfile(ctx, db, migration.legacy, migration.target); err != nil {
				return err
			}
			if migration.declared != migration.legacy {
				return store.RelocateProfile(ctx, db, migration.declared, migration.target)
			}
			return nil
		})
	}
	if err == nil {
		var closed *store.Closed
		if closed, err = handle.Pause(ctx); err == nil {
			err = closed.Close()
		}
	}
	if err := errors.Join(err, handle.Close()); err != nil {
		return err
	}
	preparedFiles, err := legacyProfileDatabaseHashes(ctx, filepath.Join(prepared, "storage"))
	if err != nil {
		return err
	}
	migration.state = legacyProfileState{
		Version: 1, Legacy: migration.legacy, Target: migration.target, Attempt: attempt,
		Node: node.ID(), Original: originalFiles, Prepared: preparedFiles,
	}
	if err := writeLegacyProfileFile(filepath.Join(original, legacyProfileMarker), []byte(attempt)); err != nil {
		return err
	}
	for _, directory := range []string{filepath.Join(prepared, "storage"), original, prepared, base} {
		if err := syncLegacyProfileDirectory(directory); err != nil {
			return err
		}
	}
	body, err := json.Marshal(migration.state)
	if err != nil {
		return err
	}
	return writeLegacyProfileFile(filepath.Join(migration.directory, "state.json"), body)
}

func (migration *legacyProfileMigration) resume(ctx context.Context, source *core.Profile, checkpoint func(string) error) (failure error) {
	state := migration.state
	staged := filepath.Join(migration.directory, "profile")
	installed := filepath.Join(migration.directory, "installed")
	if migration.matchesFile(filepath.Join(migration.directory, "done"), state.Attempt) {
		if exists, err := legacyProfileExists(migration.target); err != nil {
			return err
		} else if !exists {
			return fmt.Errorf("completed legacy migration target %q is missing; retained original databases are at %q", migration.target, migration.backup())
		}
		return migration.cleanup()
	}
	if exists, err := legacyProfileExists(migration.target); err != nil {
		return err
	} else if exists {
		if !migration.matchesMarker(migration.target, state.Attempt) || !migration.matchesFile(installed, state.Attempt) {
			return fmt.Errorf("legacy migration target %q already exists; the unpublished profile remains at %q", migration.target, staged)
		}
		return migration.finish(ctx)
	}
	exists, err := legacyProfileExists(staged)
	if err != nil {
		return err
	}
	if !exists {
		if exists, err := legacyProfileExists(migration.legacy); err != nil {
			return err
		} else if !exists {
			return fmt.Errorf("legacy migration source %q and staging profile %q are missing; recovery files remain at %q and %q", migration.legacy, staged, migration.directory, migration.backup())
		}
		if source == nil {
			source, err = core.AcquireProfile(migration.legacy)
			if err != nil {
				return err
			}
			defer func() { failure = errors.Join(failure, source.Close()) }()
		}
		if err := migration.checkIdentity(migration.legacy); err != nil {
			return err
		}
		if err := checkLegacyProfileHashes(ctx, filepath.Join(migration.legacy, "storage"), state.Original); err != nil {
			return fmt.Errorf("legacy profile changed after migration preparation; original and prepared copies remain at %q: %w", migration.directory, err)
		}
		if err := migration.keepOriginal(ctx); err != nil {
			return err
		}
		marker := filepath.Join(migration.legacy, legacyProfileMarker)
		if !migration.matchesFile(marker, state.Attempt) {
			if err := writeLegacyProfileFile(marker, []byte(state.Attempt)); err != nil {
				return err
			}
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		if err := renameLegacyProfile(migration.legacy, staged); err != nil {
			return err
		}
		if err := errors.Join(syncLegacyProfileDirectory(filepath.Dir(migration.legacy)), syncLegacyProfileDirectory(migration.directory)); err != nil {
			return err
		}
	} else {
		if exists, err := legacyProfileExists(migration.legacy); err != nil || exists {
			return errors.Join(err, fmt.Errorf("legacy path %q was recreated; the unpublished profile remains at %q", migration.legacy, staged))
		}
		if !migration.matchesMarker(staged, state.Attempt) {
			return fmt.Errorf("unrecognized legacy migration staging directory at %q", staged)
		}
		source, err = core.AcquireProfile(staged)
		if err != nil {
			return err
		}
		defer func() { failure = errors.Join(failure, source.Close()) }()
	}
	if err := migration.checkIdentity(staged); err != nil {
		return err
	}
	if checkpoint != nil {
		if err := checkpoint("staged"); err != nil {
			return err
		}
	}
	if !migration.matchesFile(installed, state.Attempt) {
		if exists, err := legacyProfileExists(installed); err != nil || exists {
			return errors.Join(err, fmt.Errorf("incomplete legacy migration installation marker at %q", installed))
		}
		if err := migration.install(ctx, staged); err != nil {
			return err
		}
		if err := writeLegacyProfileFile(installed, []byte(state.Attempt)); err != nil {
			return err
		}
	}
	if err := checkLegacyProfileHashes(ctx, filepath.Join(staged, "storage"), state.Prepared); err != nil {
		return err
	}
	if checkpoint != nil {
		if err := checkpoint("installed"); err != nil {
			return err
		}
	}
	if exists, err := legacyProfileExists(migration.legacy); err != nil || exists {
		return errors.Join(err, fmt.Errorf("legacy path %q was recreated; the complete profile remains at %q", migration.legacy, staged))
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := renameLegacyProfile(staged, migration.target); err != nil {
		return err
	}
	if err := errors.Join(syncLegacyProfileDirectory(filepath.Dir(migration.target)), syncLegacyProfileDirectory(migration.directory)); err != nil {
		return err
	}
	if checkpoint != nil {
		if err := checkpoint("published"); err != nil {
			return err
		}
	}
	return migration.finish(ctx)
}

func (migration *legacyProfileMigration) install(ctx context.Context, staged string) error {
	prepared := filepath.Join(migration.directory, "preparation", "prepared", "storage")
	if err := checkLegacyProfileHashes(ctx, prepared, migration.state.Prepared); err != nil {
		return err
	}
	if err := checkLegacyProfileHashes(ctx, filepath.Join(migration.backup(), "storage"), migration.state.Original); err != nil {
		return err
	}
	installing := filepath.Join(migration.directory, "installing")
	if !migration.matchesFile(installing, migration.state.Attempt) {
		if err := checkLegacyProfileHashes(ctx, filepath.Join(staged, "storage"), migration.state.Original); err != nil {
			return err
		}
		if err := writeLegacyProfileFile(installing, []byte(migration.state.Attempt)); err != nil {
			return err
		}
	}
	if err := quarantineLegacyProfilePython(staged, migration.backup()); err != nil {
		return err
	}
	// The complete original family is retained in the named backup. Replacing
	// only native members can restart after any partial prepared-family copy.
	storage := filepath.Join(staged, "storage")
	for _, name := range legacyProfileDatabaseNames() {
		path := filepath.Join(storage, name)
		if exists, err := legacyProfileExists(path); err != nil {
			return err
		} else if !exists {
			continue
		}
		if _, err := legacyProfileRegular(path); err != nil {
			return err
		}
		if err := os.Remove(path); err != nil {
			return err
		}
	}
	if err := syncLegacyProfileDirectory(storage); err != nil {
		return err
	}
	if _, err := copyLegacyProfileDatabases(ctx, prepared, storage); err != nil {
		return err
	}
	return syncLegacyProfileDirectory(storage)
}

func (migration *legacyProfileMigration) finish(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := migration.checkIdentity(migration.target); err != nil {
		return err
	}
	if err := writeLegacyProfileFile(filepath.Join(migration.directory, "done"), []byte(migration.state.Attempt)); err != nil {
		return err
	}
	return migration.cleanup()
}

func (migration *legacyProfileMigration) checkIdentity(path string) error {
	if _, err := legacyProfileRegular(filepath.Join(path, "link.key")); err != nil {
		return err
	}
	node, err := link.ReadIdentity(path)
	if err != nil {
		return err
	}
	if node.ID() != migration.state.Node {
		return fmt.Errorf("legacy migration identity changed at %q", path)
	}
	return nil
}

func (migration *legacyProfileMigration) matchesMarker(path, token string) bool {
	info, err := os.Lstat(path)
	return err == nil && info.IsDir() && migration.matchesFile(filepath.Join(path, legacyProfileMarker), token)
}

func (*legacyProfileMigration) matchesFile(path, token string) bool {
	if token == "" {
		return false
	}
	file, err := os.Lstat(path)
	if err != nil || !file.Mode().IsRegular() || file.Size() != int64(len(token)) {
		return false
	}
	body, err := os.ReadFile(path)
	return err == nil && string(body) == token
}

func legacyProfileReadJSON(path string, target any) error {
	info, err := legacyProfileRegular(path)
	if err != nil {
		return err
	}
	if info.Size() > 64*1024 {
		return fmt.Errorf("legacy migration journal is too large at %q", path)
	}
	body, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	return json.Unmarshal(body, target)
}

func legacyProfileExists(path string) (bool, error) {
	_, err := os.Lstat(path)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	return err == nil, err
}

func legacyProfileRegular(path string) (os.FileInfo, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() {
		return nil, &core.InvalidProfileError{Path: path, Reason: "legacy migration requires a regular file"}
	}
	if err := validateLegacyProfileFile(path, info); err != nil {
		return nil, err
	}
	return info, nil
}

func legacyProfileDatabaseNames() []string {
	var names []string
	for _, name := range []string{"node.turso", "knowledge.turso"} {
		names = append(names, core.DatabaseFiles(name)...)
		// Preserve SQLite crash journals too; never mistake them for ordinary data.
		names = append(names, name+"-journal", name+"-shm")
	}
	return names
}

func legacyProfileDatabaseName(name string) bool {
	for _, candidate := range legacyProfileDatabaseNames() {
		if name == candidate {
			return true
		}
	}
	return false
}

func legacyProfileDatabaseHashes(ctx context.Context, storage string) (map[string]string, error) {
	files := make(map[string]string)
	for _, name := range legacyProfileDatabaseNames() {
		path := filepath.Join(storage, name)
		info, err := legacyProfileRegular(path)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return nil, err
		}
		if name == "node.turso" && info.Size() == 0 {
			return nil, fmt.Errorf("legacy Node database is empty at %q", path)
		}
		file, err := os.Open(path)
		if err != nil {
			return nil, err
		}
		hash := sha256.New()
		_, copyErr := io.Copy(hash, &databaseMigrationReader{ctx: ctx, reader: file})
		if err := errors.Join(copyErr, file.Close()); err != nil {
			return nil, err
		}
		files[name] = hex.EncodeToString(hash.Sum(nil))
	}
	if files["node.turso"] == "" {
		return nil, fmt.Errorf("legacy profile has no Node database at %q", storage)
	}
	if files["knowledge.turso"] == "" {
		for name := range files {
			if strings.HasPrefix(name, "knowledge.") {
				return nil, fmt.Errorf("legacy knowledge database is missing at %q", storage)
			}
		}
	}
	return files, nil
}

func checkLegacyProfileHashes(ctx context.Context, storage string, expected map[string]string) error {
	actual, err := legacyProfileDatabaseHashes(ctx, storage)
	if err != nil {
		return err
	}
	if len(actual) != len(expected) {
		return fmt.Errorf("legacy migration database family changed at %q", storage)
	}
	for name, sum := range expected {
		if actual[name] != sum {
			return fmt.Errorf("legacy migration database changed at %q", filepath.Join(storage, name))
		}
	}
	return nil
}

func copyLegacyProfileDatabases(ctx context.Context, source, target string) (map[string]string, error) {
	files, err := legacyProfileDatabaseHashes(ctx, source)
	if err != nil {
		return nil, err
	}
	for name, expected := range files {
		sum, err := copyLegacyProfileFile(ctx, filepath.Join(source, name), filepath.Join(target, name))
		if err != nil {
			return nil, err
		}
		if sum != expected {
			return nil, fmt.Errorf("legacy migration source changed at %q", filepath.Join(source, name))
		}
	}
	if err := syncLegacyProfileDirectory(target); err != nil {
		return nil, err
	}
	return files, nil
}

func copyLegacyProfileFile(ctx context.Context, source, target string) (string, error) {
	before, err := legacyProfileRegular(source)
	if err != nil {
		return "", err
	}
	input, err := os.Open(source)
	if err != nil {
		return "", err
	}
	defer input.Close()
	opened, err := input.Stat()
	if err != nil {
		return "", err
	}
	if !os.SameFile(before, opened) {
		return "", fmt.Errorf("legacy migration source changed at %q", source)
	}
	output, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return "", err
	}
	hash := sha256.New()
	count, copyErr := io.Copy(io.MultiWriter(output, hash), &databaseMigrationReader{ctx: ctx, reader: input})
	after, statErr := input.Stat()
	if statErr == nil && (count != before.Size() || after.Size() != before.Size() || !after.ModTime().Equal(before.ModTime())) {
		copyErr = errors.Join(copyErr, fmt.Errorf("legacy migration source changed at %q", source))
	}
	if err := errors.Join(copyErr, statErr, output.Sync(), output.Close()); err != nil {
		return "", err
	}
	return hex.EncodeToString(hash.Sum(nil)), nil
}

func writeLegacyProfileFile(path string, body []byte) error {
	temporary := filepath.Join(filepath.Dir(path), ".write-"+uuid.NewString())
	file, err := os.OpenFile(temporary, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return err
	}
	_, writeErr := file.Write(body)
	if err := errors.Join(writeErr, file.Sync(), file.Close()); err != nil {
		return err
	}
	if err := renameLegacyProfile(temporary, path); err != nil {
		return err
	}
	return syncLegacyProfileDirectory(filepath.Dir(path))
}

type databaseMigrationReader struct {
	ctx    context.Context
	reader io.Reader
}

func (reader *databaseMigrationReader) Read(buffer []byte) (int, error) {
	if err := reader.ctx.Err(); err != nil {
		return 0, err
	}
	return reader.reader.Read(buffer)
}

func (migration *legacyProfileMigration) backup() string {
	return migration.target + ".before-legacy-migration-databases"
}

func (migration *legacyProfileMigration) ownsPreparation(attempt string) bool {
	var owner legacyProfileState
	if err := legacyProfileReadJSON(filepath.Join(migration.directory, "preparation", "owner.json"), &owner); err != nil {
		return false
	}
	return owner.Version == 1 && owner.Legacy == migration.legacy && owner.Target == migration.target &&
		strings.HasPrefix(owner.Attempt, "attempt-") && (attempt == "" || owner.Attempt == attempt)
}

func (migration *legacyProfileMigration) keepOriginal(ctx context.Context) error {
	backup := migration.backup()
	if exists, err := legacyProfileExists(backup); err != nil {
		return err
	} else if !exists {
		if err := renameLegacyProfile(filepath.Join(migration.directory, "preparation", "original"), backup); err != nil {
			return err
		}
		if err := errors.Join(syncLegacyProfileDirectory(filepath.Dir(backup)), syncLegacyProfileDirectory(filepath.Join(migration.directory, "preparation"))); err != nil {
			return err
		}
	}
	if !migration.matchesMarker(backup, migration.state.Attempt) {
		return fmt.Errorf("unrecognized original legacy database backup at %q", backup)
	}
	return checkLegacyProfileHashes(ctx, filepath.Join(backup, "storage"), migration.state.Original)
}

func (migration *legacyProfileMigration) cleanup() error {
	base := filepath.Join(migration.directory, "preparation")
	if exists, err := legacyProfileExists(base); err != nil {
		return err
	} else if exists {
		if !migration.ownsPreparation(migration.state.Attempt) {
			return fmt.Errorf("unrecognized legacy preparation directory at %q", base)
		}
		if err := os.RemoveAll(base); err != nil {
			return err
		}
	}
	marker := filepath.Join(migration.target, legacyProfileMarker)
	if migration.matchesFile(marker, migration.state.Attempt) {
		if err := os.Remove(marker); err != nil {
			return err
		}
		if err := syncLegacyProfileDirectory(migration.target); err != nil {
			return err
		}
	}
	return syncLegacyProfileDirectory(migration.directory)
}
