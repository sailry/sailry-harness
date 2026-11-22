package settings

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
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

func TestBackupArchiveSelectionUnarchivesFromContextMenu(t *testing.T) {
	var singles []string
	var batches [][]string
	m := &Model{
		Archived: []viewmodel.Archived{
			{ID: "first", Title: "First archive"},
			{ID: "second", Title: "Second archive"},
			{ID: "third", Title: "Third archive"},
			{ID: "fourth", Title: "Fourth archive"},
		},
		BackupActions: &BackupActions{
			Unarchive:     func(id string) { singles = append(singles, id) },
			UnarchiveMany: func(ids []string) { batches = append(batches, slices.Clone(ids)) },
		},
	}
	window := backupArchiveTester(m, nil)
	manage := i18n.In("en", "backup_manage")
	unarchive := i18n.In("en", "backup_unarchive")
	clickArchiveControl(t, window, manage, 0)
	clickArchiveControl(t, window, "First archive", 0)
	clickArchiveControl(t, window, "Fourth archive", ui.Shift)
	clickArchiveControl(t, window, "Second archive", ui.Cmd)
	chooseArchiveMenu(t, window, "Third archive", unarchive)
	if len(batches) != 1 || !slices.Equal(batches[0], []string{"first", "third", "fourth"}) || len(singles) != 0 {
		t.Fatalf("restoring the selection: batches=%v, singles=%v", batches, singles)
	}
	chooseArchiveMenu(t, window, "Second archive", unarchive)
	if !slices.Equal(singles, []string{"second"}) || len(batches) != 1 {
		t.Fatalf("restoring the unselected row: batches=%v, singles=%v", batches, singles)
	}

	clickArchiveControl(t, window, "Fourth archive", ui.Cmd)
	clickArchiveControl(t, window, i18n.In("en", "close"), 0)
	clickArchiveControl(t, window, manage, 0)
	chooseArchiveMenu(t, window, "Fourth archive", unarchive)
	if !slices.Equal(singles, []string{"second", "fourth"}) || len(batches) != 1 {
		t.Fatalf("selection persisted after closing the dialog: batches=%v, singles=%v", batches, singles)
	}
}

func TestBackupArchiveBatchDeleteConfirmsOnce(t *testing.T) {
	var prepares, removals [][]string
	var prompts []kit.Confirmation
	var answer func(int)
	m := &Model{
		Archived: []viewmodel.Archived{
			{ID: "first", Title: "First archive"},
			{ID: "second", Title: "Second archive", Private: true},
			{ID: "other", Title: "Other archive"},
		},
		BackupActions: &BackupActions{Host: "Test Node", PrepareDeleteMany: func(ids []string) func() {
			captured := slices.Clone(ids)
			prepares = append(prepares, captured)
			return func() { removals = append(removals, captured) }
		}},
	}
	window := backupArchiveTester(m, func(request kit.Confirmation, done func(int)) {
		prompts = append(prompts, request)
		answer = done
	})
	clickArchiveControl(t, window, i18n.In("en", "backup_manage"), 0)
	clickArchiveControl(t, window, "First archive", 0)
	clickArchiveControl(t, window, "Second archive", ui.Cmd)
	deleteLabel := i18n.In("en", "backup_delete")
	chooseArchiveMenu(t, window, "First archive", deleteLabel)
	wantIDs := []string{"first", "second"}
	wantMessage := i18n.In("en", "backup_action_host", "name", "Test Node") + "\n\n" +
		i18n.In("en", "session_delete_many_confirm", "count", len(wantIDs))
	if len(prepares) != 1 || !slices.Equal(prepares[0], wantIDs) || len(prompts) != 1 || prompts[0].Message != wantMessage || len(removals) != 0 {
		t.Fatalf("opening deletion: prepares=%v, prompts=%v, removals=%v", prepares, prompts, removals)
	}
	answer(0)
	window.Frame()
	if len(removals) != 0 || len(prompts) != 1 || m.backups.removing {
		t.Fatalf("canceling deletion: prompts=%v, removals=%v, open=%t", prompts, removals, m.backups.removing)
	}
	chooseArchiveMenu(t, window, "Second archive", deleteLabel)
	if len(prepares) != 2 || !slices.Equal(prepares[1], wantIDs) || len(prompts) != 2 || prompts[1].Message != wantMessage || len(removals) != 0 {
		t.Fatalf("reopening deletion: prepares=%v, prompts=%v, removals=%v", prepares, prompts, removals)
	}
	m.Archived = m.Archived[1:]
	window.Frame()
	answer(1)
	window.Frame()
	window.Frame()
	if len(removals) != 1 || !slices.Equal(removals[0], wantIDs) || len(prompts) != 2 || m.backups.removing {
		t.Fatalf("confirming deletion: prompts=%v, removals=%v, open=%t", prompts, removals, m.backups.removing)
	}
}

func TestBackupArchiveIndividualAndAllDelete(t *testing.T) {
	var prepares, removals []string
	var prompts []kit.Confirmation
	var answer func(int)
	m := &Model{
		Archived: []viewmodel.Archived{
			{ID: "private", Title: "Private archive", Private: true},
			{ID: "project", Title: "Project archive"},
		},
		BackupActions: &BackupActions{PrepareDelete: func(id string) func() {
			prepares = append(prepares, id)
			return func() { removals = append(removals, id) }
		}},
	}
	window := backupArchiveTester(m, func(request kit.Confirmation, done func(int)) {
		prompts = append(prompts, request)
		answer = done
	})
	clickArchiveControl(t, window, i18n.In("en", "backup_manage"), 0)
	clickArchiveControl(t, window, "Private archive", 0)
	clickArchiveControl(t, window, "Project archive", ui.Cmd)
	clickArchiveControl(t, window, i18n.In("en", "backup_delete"), 0)
	wantMessage := i18n.In("en", "session_delete_private_named", "name", "Private archive")
	if !slices.Equal(prepares, []string{"private"}) || len(prompts) != 1 || prompts[0].Message != wantMessage || len(removals) != 0 {
		t.Fatalf("opening single deletion: prepares=%v, prompts=%v, removals=%v", prepares, prompts, removals)
	}
	answer(1)
	window.Frame()
	clickArchiveControl(t, window, i18n.In("en", "close"), 0)
	clickArchiveControl(t, window, i18n.In("en", "backup_delete_all_action"), 0)
	if !slices.Equal(prepares, []string{"private", ""}) || len(prompts) != 2 || !slices.Equal(removals, []string{"private"}) {
		t.Fatalf("opening all deletion: prepares=%v, prompts=%v, removals=%v", prepares, prompts, removals)
	}
	answer(1)
	window.Frame()
	if !slices.Equal(removals, []string{"private", ""}) {
		t.Fatalf("confirming all deletion: removals=%v", removals)
	}
}

func backupArchiveTester(m *Model, present kit.ConfirmationPresenter) *ui.Tester {
	window := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		kit.BindConfirmations(c, present)
		ui.Column(c).Fill().Children(func() { backup(c, m) })
	}, 760, 680)
	window.SetLocale("en")
	window.Frame()
	return window
}

func clickArchiveControl(t *testing.T, window *ui.Tester, title string, mods ui.Modifiers) {
	t.Helper()
	if err := window.ClickWith(mods, title); err != nil {
		t.Fatal(err)
	}
	window.Frame()
}

func chooseArchiveMenu(t *testing.T, window *ui.Tester, title, item string) {
	t.Helper()
	if err := window.RightClick(title); err != nil {
		t.Fatal(err)
	}
	if err := window.ChooseMenuItem(item); err != nil {
		t.Fatal(err)
	}
	window.Frame()
}
