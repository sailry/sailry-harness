package node

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/core"
)

func (migration *legacyProfileMigration) checkUnstaged() error {
	for _, path := range []string{migration.target, filepath.Join(migration.directory, "profile"),
		filepath.Join(migration.directory, "installing"), filepath.Join(migration.directory, "installed"), filepath.Join(migration.directory, "done")} {
		if exists, err := legacyProfileExists(path); err != nil {
			return err
		} else if exists {
			return fmt.Errorf("cannot prepare legacy migration while %q exists", path)
		}
	}
	return nil
}

// Keep state.json on the old attempt until qualification succeeds. The owned
// archive records that a refresh began and also protects the old original, even
// if keepOriginal already moved it out before a failed source rename.
func (migration *legacyProfileMigration) refresh(ctx context.Context, archive string, archived bool, checkpoint func(string) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	base := filepath.Join(migration.directory, "preparation")
	snapshot := base
	if archived {
		snapshot = archive
		if _, err := migration.checkDiscardablePreparation(base); err != nil {
			return err
		}
	}
	owner, err := migration.preparationOwner(snapshot)
	if err != nil {
		return err
	}
	if owner.Attempt != migration.state.Attempt || owner.Node != migration.state.Node {
		return fmt.Errorf("unrecognized legacy preparation directory at %q", snapshot)
	}

	original, backup := filepath.Join(snapshot, "original"), migration.backup()
	hasBackup, err := legacyProfileExists(backup)
	if err != nil {
		return err
	}
	retained := original
	if hasBackup {
		if hasOriginal, err := legacyProfileExists(original); err != nil {
			return err
		} else if archived || hasOriginal {
			return fmt.Errorf("unexpected original legacy database backup at %q", backup)
		}
		retained = backup
	}
	if err := checkLegacyProfileDirectory(retained); err != nil {
		return err
	}
	if !migration.matchesFile(filepath.Join(retained, legacyProfileMarker), migration.state.Attempt) {
		return fmt.Errorf("unrecognized original legacy database snapshot at %q", retained)
	}
	storage := filepath.Join(retained, "storage")
	if err := checkLegacyProfileDirectory(storage); err != nil {
		return err
	}
	if err := migration.checkIdentity(retained); err != nil {
		return err
	}
	if err := checkLegacyProfileHashes(ctx, storage, migration.state.Original); err != nil {
		return err
	}
	marker := filepath.Join(migration.legacy, legacyProfileMarker)
	marked, err := legacyProfileExists(marker)
	if err != nil {
		return err
	}
	if marked {
		if _, err := legacyProfileRegular(marker); err != nil {
			return err
		}
		if !migration.matchesFile(marker, migration.state.Attempt) {
			return fmt.Errorf("unrecognized legacy migration marker at %q", marker)
		}
	}
	if err := migration.checkUnstaged(); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if !archived {
		// Restore the v1 layout before archiving it. If interrupted here,
		// keepOriginal can still find the original in either v1 location.
		if hasBackup {
			if err := renameLegacyProfile(backup, original); err != nil {
				return err
			}
		}
		if err := errors.Join(syncLegacyProfileDirectory(base), syncLegacyProfileDirectory(filepath.Dir(backup))); err != nil {
			return err
		}
		if err := renameLegacyProfile(base, archive); err != nil {
			return err
		}
	}
	if err := syncLegacyProfileDirectory(migration.directory); err != nil {
		return err
	}
	if marked {
		if err := renameLegacyProfile(marker, filepath.Join(archive, "source-marker")); err != nil {
			return err
		}
	}
	if err := errors.Join(syncLegacyProfileDirectory(archive), syncLegacyProfileDirectory(migration.legacy)); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if checkpoint != nil {
		if err := checkpoint("refreshing"); err != nil {
			return err
		}
	}
	return migration.prepareAndResume(ctx, checkpoint)
}

func (migration *legacyProfileMigration) preparationOwner(base string) (legacyProfilePreparationOwner, error) {
	var owner legacyProfilePreparationOwner
	if err := checkLegacyProfileDirectory(base); err != nil {
		return owner, err
	}
	if err := legacyProfileReadJSON(filepath.Join(base, "owner.json"), &owner); err != nil {
		return owner, err
	}
	if owner.Version != 1 || owner.Legacy != migration.legacy || owner.Target != migration.target ||
		!strings.HasPrefix(owner.Attempt, "attempt-") || owner.Attempt == owner.Replaces {
		return owner, fmt.Errorf("unrecognized legacy preparation directory at %q", base)
	}
	return owner, nil
}

func checkLegacyProfileDirectory(path string) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	if info.Mode().Type() != os.ModeDir {
		return &core.InvalidProfileError{Path: path, Reason: "legacy migration requires a directory without links"}
	}
	return nil
}

func (migration *legacyProfileMigration) checkDiscardablePreparation(base string) (bool, error) {
	if exists, err := legacyProfileExists(base); err != nil || !exists {
		return false, err
	}
	owner, err := migration.preparationOwner(base)
	if err != nil {
		return false, err
	}
	state := migration.state
	if owner.Replaces != state.Attempt || (state.Attempt != "" && owner.Node != state.Node) {
		return false, fmt.Errorf("unrecognized legacy preparation directory at %q", base)
	}
	return true, nil
}

func (migration *legacyProfileMigration) beginPreparation(base string, owner legacyProfilePreparationOwner) error {
	if exists, err := migration.checkDiscardablePreparation(base); err != nil {
		return err
	} else if exists {
		if err := discardLegacyProfilePreparation(base); err != nil {
			return err
		}
	}
	body, err := json.Marshal(owner)
	if err != nil {
		return err
	}
	// Publish the directory with its owner already inside. A process that dies
	// earlier can leave only an unreferenced temporary directory, not an ownerless
	// preparation at the recovery path.
	temporary, err := os.MkdirTemp(migration.directory, ".preparation-")
	if err != nil {
		return err
	}
	if err := core.PrivateDirectory(temporary); err != nil {
		return errors.Join(err, os.RemoveAll(temporary))
	}
	if err := writeLegacyProfileFile(filepath.Join(temporary, "owner.json"), body); err != nil {
		return errors.Join(err, os.RemoveAll(temporary))
	}
	if err := renameLegacyProfile(temporary, base); err != nil {
		return errors.Join(err, os.RemoveAll(temporary))
	}
	return syncLegacyProfileDirectory(migration.directory)
}

func discardLegacyProfilePreparation(base string) error {
	parent := filepath.Dir(base)
	discarded := filepath.Join(parent, ".discarded-preparation-"+uuid.NewString())
	// Retire the recovery path before any recursive delete can remove its owner.
	if err := renameLegacyProfile(base, discarded); err != nil {
		return err
	}
	if err := syncLegacyProfileDirectory(parent); err != nil {
		return err
	}
	return os.RemoveAll(discarded)
}
