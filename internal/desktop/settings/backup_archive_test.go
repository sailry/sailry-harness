package settings

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestBackupArchiveGuardKeepsManagementAvailable(t *testing.T) {
	calls := 0
	m := &Model{Sessions: 1, BackupActions: &BackupActions{ArchiveBlocked: true, ArchiveAll: func() { calls++ }}}
	window := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { backupArchiveRows(c, m) })
	}, 700, 400)
	window.SetLocale("en")
	window.Frame()
	archive := i18n.In("en", "backup_archive_all_action")
	_ = window.Click(archive)
	window.Frame()
	if calls != 0 {
		t.Fatal("archive all ran while session tasks were unfinished")
	}
	if err := window.Click(i18n.In("en", "backup_manage")); err != nil {
		t.Fatal(err)
	}
	if !m.backups.archives {
		t.Fatal("unfinished tasks blocked management of existing archives")
	}
	m.BackupActions.ArchiveBlocked = false
	window.Frame()
	if err := window.Click(archive); err != nil {
		t.Fatal(err)
	}
	if calls != 1 {
		t.Fatal("archive all stayed blocked after session tasks finished")
	}
}
