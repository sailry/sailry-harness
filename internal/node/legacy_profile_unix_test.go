//go:build unix

package node

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestMigrateLegacyProfileRejectsHardLinkedDatabase(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	source := filepath.Join(fixture.legacy, "storage", "node.turso")
	alias := filepath.Join(filepath.Dir(fixture.legacy), "database-hardlink")
	if err := os.Link(source, alias); err != nil {
		t.Fatal(err)
	}
	before, err := os.ReadFile(alias)
	if err != nil {
		t.Fatal(err)
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err == nil {
		t.Fatal("migration accepted a hard-linked source database")
	}
	after, err := os.ReadFile(alias)
	if err != nil || !bytes.Equal(before, after) {
		t.Fatalf("migration modified the external hard link: %v", err)
	}
}

func TestMigrateLegacyProfileRejectsRootInternalAbsoluteLink(t *testing.T) {
	t.Parallel()
	fixture := newLegacyMigrationFixture(t)
	path := filepath.Join(fixture.legacy, "linked-notes")
	target := filepath.Join(fixture.legacy, "notes")
	if err := os.Symlink(target, path); err != nil {
		t.Fatal(err)
	}
	if err := MigrateLegacyProfile(t.Context(), fixture.legacy, fixture.target); err == nil || !strings.Contains(err.Error(), "profile-relative") {
		t.Fatalf("unsafe absolute-link migration: %v", err)
	}
	if actual, err := os.Readlink(path); err != nil || actual != target {
		t.Fatalf("refused migration changed the source link: %q, %v", actual, err)
	}
	if exists, err := legacyProfileExists(fixture.target); err != nil || exists {
		t.Fatalf("refused migration created target: %v", err)
	}
}
