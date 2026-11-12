package settings

import (
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

var backupMessages = Messages{"backup_saved", "backup_failed", "backup_conflict", "backup_unknown"}

// backupDialogs are the Backup page's dialogs: the storage editor with
// its draft, the archived sessions and deleting every session. The live
// page adds the stored backups, the restore form and deleting one session.
type backupDialogs struct {
	editing, archives, deleting bool
	draft                       BackupTarget
	// unconfirmed is whether the storage dialog's save did not succeed;
	// with BackupActions.Retry its draft is locked and Save reads Retry.
	unconfirmed bool
	// keeping is the retention awaiting its confirmation.
	keeping               bool
	keep                  int
	records, restoring    bool
	restore               BackupRestore
	removing              bool
	removed, removedTitle string
	// restarting offers the restart on the profile just restored at restart.
	restarting bool
	restart    string
}

// Check validates the storage draft: the i18n key of the problem, or "".
func (b *BackupTarget) Check() string {
	switch {
	case b.Tab < 2 && strings.TrimSpace(b.Endpoint) == "", b.Tab == 1 && strings.TrimSpace(b.Bucket) == "",
		b.Tab == 2 && strings.TrimSpace(b.Directory) == "":
		return "backup_invalid"
	case b.Passphrase == "" && !b.PassphraseSet:
		return "backup_passphrase_required"
	case b.Passphrase != "" && len([]rune(b.Passphrase)) < 8:
		return "backup_passphrase_invalid"
	case b.Passphrase != b.Confirm:
		return "backup_passphrase_mismatch"
	}
	return ""
}

// BackupKeeps are the retentions offered: every backup, or the last few.
var BackupKeeps = []int{0, 3, 7, 14, 30}

// backupKeepOptions are the retention's choices and their labels; a count
// the Node holds that the page does not offer is listed with them.
func backupKeepOptions(c *ui.Context, saved int) (counts []int, labels []string) {
	counts = BackupKeeps
	if !slices.Contains(counts, saved) {
		counts = append(slices.Clone(counts), saved)
		slices.Sort(counts)
	}
	for _, n := range counts {
		label := i18n.T(c, "backup_keep_all")
		if n > 0 {
			label = i18n.T(c, "backup_keep_last", "count", n)
		}
		labels = append(labels, label)
	}
	return counts, labels
}

// backupLast is the last backup's row: when it ended, what it left out,
// and its outcome; the Node's reason for a failure is the outcome's tooltip.
func backupLast(c *ui.Context, last BackupLast) {
	if last.At.IsZero() {
		return
	}
	p := theme.Of(c)
	detail := last.At.Local().Format("2006-01-02 15:04")
	if last.Skipped > 0 {
		detail += " · " + i18n.T(c, "backup_last_skipped", "count", last.Skipped)
	}
	kit.Row(c, "last", i18n.T(c, "backup_last"), detail, func() {
		if last.Problem == "" {
			ui.Text(c, i18n.T(c, "backup_last_done")).FontSize(kit.RowFont).TextColor(p.MutedForeground)
			return
		}
		outcome := ui.Text(c, i18n.T(c, last.Problem)).FontSize(kit.RowFont).TextColor(p.Danger)
		if last.Reason != "" {
			kit.Tooltip(c, outcome, last.Reason)
		}
	})
}

// backup is the storage and the schedule, exporting, backing up and
// restoring, and the sessions' archive and deletion. Exports, backups and
// restores read and write archives, and the sessions are the user's: the
// preview stops at the notice for them.
func backup(c *ui.Context, m *Model) {
	b := &m.Backup
	d := &m.backups
	a := m.BackupActions
	t := func(k string) string { return i18n.T(c, k) }
	target := b.Saved.PassphraseSet && (b.Saved.Tab == 2 && b.Saved.Directory != "" || b.Saved.Tab < 2 && b.Saved.Endpoint != "")
	entry := func(key, title, action string, run func()) {
		kit.Entry(c, key, func() { ui.Text(c, title).FontSize(14) }, nil, func() {
			kit.TextActionBusy(c.Key("action"), action, b.Pending, m.Busy[BusyBackup+key]).OnClick(run)
		})
	}
	configure := func() {
		d.draft, d.unconfirmed = b.Saved, false
		d.draft.Passphrase, d.draft.Confirm, d.draft.Identity, d.draft.Secret, d.draft.Token = "", "", "", "", ""
		d.editing = true
	}
	kit.Group(c, "backup", "", "", nil, func() {
		kit.Card(c, func() {
			entry("storage", t("backup_destination"), t("backup_configure"), configure)
			// The live page saves the schedule's draft whenever it changes.
			if kit.SwitchRow(c, "schedule", t("backup_schedule_enabled"), "", &b.Draft.Schedule) && a == nil {
				report(c, b.Commit(), Messages{Failed: backupMessages.Failed, Conflict: backupMessages.Conflict, Uncertain: backupMessages.Uncertain})
			}
			if b.Draft.Schedule {
				kit.Row(c, "time", t("backup_schedule_time"), "", func() {
					ui.TimeInput(c.Key("time"), &b.Draft.Time).Label(t("backup_schedule_time"))
				})
				kit.Row(c, "timezone", t("backup_timezone"), "", func() {
					ui.TextInput(c.Key("input"), &b.Draft.Timezone).Label(t("backup_timezone")).Width(192)
				})
			}
			counts, labels := backupKeepOptions(c, b.Saved.Keep)
			chosen := slices.Index(counts, b.Saved.Keep)
			kit.Row(c, "keep", t("backup_keep"), "", func() {
				if !pick(c, "keep", t("backup_keep"), &chosen, labels, b.Pending).Width(160).Changed() {
					return
				}
				// Keeping fewer deletes stored backups: confirmed first.
				// Keeping all deletes nothing and is saved at once.
				if d.keep = counts[chosen]; d.keep > 0 {
					d.keeping = true
					return
				}
				m.keepBackups(c, 0)
			})
			export := t("backup_configure")
			if b.Saved.PassphraseSet {
				export = t("backup_export")
			}
			entry("export", t("backup_export_data"), export, func() {
				if !b.Saved.PassphraseSet {
					configure()
					return
				}
				if a == nil {
					notice(c)
					return
				}
				a.Export()
			})
			now := t("backup_configure")
			if target {
				now = t("backup_now")
			}
			entry("now", t("backup_now"), now, func() {
				if !target {
					configure()
					return
				}
				if a == nil {
					notice(c)
					return
				}
				a.Now()
			})
			backupLast(c, b.Saved.Last)
			entry("import", t("backup_import"), t("backup_restore"), func() {
				if a == nil {
					notice(c)
					return
				}
				a.Import(func(path string) { d.openRestore(BackupRestore{File: path}) })
			})
			kit.Entry(c, "storage-restore", func() { ui.Text(c, t("backup_restore_storage")).FontSize(14) }, nil, func() {
				kit.TextAction(c.Key("action"), t("backup_restore"), b.Pending || !target).OnClick(func() {
					if a == nil {
						notice(c)
						return
					}
					d.records = true
					a.List()
				})
			})
			entry("archives", t("backup_archived_sessions"), t("backup_manage"), func() { d.archives = true })
			kit.Entry(c, "archive-all", func() { ui.Text(c, t("backup_archive_all")).FontSize(14) }, nil, func() {
				kit.TextActionBusy(c.Key("action"), t("backup_archive_all_action"), b.Pending || m.Sessions == 0, m.Busy[BusyBackup+"archive"]).OnClick(func() {
					if a == nil {
						notice(c)
						return
					}
					a.ArchiveAll()
				})
			})
			kit.Entry(c, "delete-all", func() { ui.Text(c, t("backup_delete_all")).FontSize(14) }, func() {
				kit.DangerBusy(c.Key("action"), t("backup_delete_all_action"), m.Busy[BusyBackup+"delete"]).Height(28).Disabled(b.Pending || m.Busy[BusyBackup+"delete"]).OnClick(func() { d.deleting = true })
			}, nil)
		})
	})
	storageDialog(c, m)
	archivesDialog(c, m)
	if d.deleting {
		// Destructive: named and its consequence said once.
		if kit.AlertDialog(c.Key("delete-all"), &d.deleting, t("backup_delete_all")+"?", t("backup_delete_all_consequence"),
			t("settings_cancel"), t("backup_delete_all_action")) == 1 {
			if a == nil {
				notice(c)
			} else {
				a.DeleteAll()
			}
		}
	}
	// Destructive: the backups are named and their deletion said once.
	if confirm(c, "keep", &d.keeping, i18n.T(c, "backup_keep_confirm", "count", d.keep), t("backup_keep_consequence"), t("settings_save")) {
		m.keepBackups(c, d.keep)
	}
	recordsDialog(c, m)
	restoreDialog(c, m)
	// The consequence for the profile in use is said once.
	if confirm(c, "restart-restored", &d.restarting, t("backup_restart_restored")+"?", t("backup_restart_consequence"), t("updates_restart")) && a != nil {
		a.Restart(d.restart)
	}
	if d.removing && a != nil {
		// Destructive: the session is named and its consequence said once.
		if kit.AlertDialog(c.Key("delete-session"), &d.removing, d.removedTitle+"?", t("backup_delete_session_consequence"),
			t("settings_cancel"), t("backup_delete")) == 1 {
			a.Delete(d.removed)
		}
	}
}

// keepBackups saves the retention. The live page's Node saves it and its
// outcome is a toast; the preview keeps it in memory.
func (m *Model) keepBackups(c *ui.Context, keep int) {
	if a := m.BackupActions; a != nil {
		a.Keep(keep)
		return
	}
	m.Backup.Saved.Keep, m.Backup.Draft.Keep = keep, keep
	report(c, m.Backup.Commit(), backupMessages)
}

// storageDialog is WebDAV, S3 or a local folder, their fields, and the
// encryption passphrase. Saved secrets are never shown: their inputs are
// empty with a placeholder keeping them.
func storageDialog(c *ui.Context, m *Model) {
	d := &m.backups
	if !d.editing {
		return
	}
	t := func(k string) string { return i18n.T(c, k) }
	x := &d.draft
	title := t("backup_destination")
	// A save the Node has not confirmed keeps its draft as it was sent.
	retry := m.BackupActions != nil && m.BackupActions.Retry && d.unconfirmed
	action := t("settings_save")
	if retry {
		action = t("plugins_retry")
	}
	kit.Dialog(c.Key("storage"), &d.editing, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Children(func() {
			kit.Segmented(c, "tabs", &x.Tab, []string{t("backup_webdav"), t("backup_s3"), t("backup_local")}, kit.SegmentedProps{Block: true, Disabled: retry}).Label(title)
			input := func(key string, value *string, secret, kept bool) {
				label := t(key)
				kit.Field(c, key, label, "", func() {
					in := ui.TextInput(c.Key("input"), value).Label(label).FillWidth().Disabled(retry)
					if secret {
						in.Password()
					}
					// The input keeps its last placeholder, so cleared
					// credentials reset it.
					placeholder := ""
					if kept {
						placeholder = t("backup_keep_secret")
					}
					in.Placeholder(placeholder)
				})
			}
			if x.Tab == 2 {
				label := t("backup_local_directory")
				kit.Field(c, "directory", label, "", func() {
					ui.Row(c).Gap(8).Children(func() {
						ui.TextInput(c.Key("input"), &x.Directory).Label(label).ReadOnly(true).Grow(1)
						// The folder picker browses the host's disk.
						kit.Secondary(c.Key("choose"), t("backup_choose_directory")).Disabled(retry).OnClick(func() {
							if a := m.BackupActions; a != nil {
								a.ChooseFolder(x.Directory, func(path string) { x.Directory = path })
								return
							}
							notice(c)
						})
					})
				})
			} else {
				input("backup_endpoint", &x.Endpoint, false, false)
				input("backup_prefix", &x.Prefix, false, false)
				if x.Tab == 1 {
					input("backup_bucket", &x.Bucket, false, false)
					input("backup_region", &x.Region, false, false)
				}
				input("backup_identity", &x.Identity, false, x.CredentialsSet)
				input("backup_secret", &x.Secret, true, x.CredentialsSet)
				if x.Tab == 1 {
					input("backup_session_token", &x.Token, true, x.CredentialsSet)
				}
			}
			input("backup_passphrase", &x.Passphrase, true, x.PassphraseSet)
			input("backup_confirm_passphrase", &x.Confirm, true, x.PassphraseSet)
			if x.CredentialsSet && x.Tab < 2 {
				kit.TextAction(c.Key("clear"), t("backup_clear_credentials"), retry).AlignSelf(ui.Start).OnClick(func() {
					x.CredentialsSet = false
				})
			}
		})
		footer(c, action, func() { d.editing = false }, func() {
			if problem := x.Check(); problem != "" {
				kit.Toast(c, kit.Failed, t(problem))
				return
			}
			if a := m.BackupActions; a != nil {
				// The draft, typed secrets included, stays in the dialog
				// until the Node has saved it.
				if m.Backup.Pending {
					return
				}
				a.Save(*x, func(ok bool) {
					d.unconfirmed = !ok
					if ok {
						d.editing, d.draft = false, BackupTarget{}
					}
				})
				return
			}
			saved := *x
			saved.PassphraseSet = saved.PassphraseSet || saved.Passphrase != ""
			saved.CredentialsSet = saved.CredentialsSet || saved.Identity != "" || saved.Secret != ""
			saved.Passphrase, saved.Confirm, saved.Identity, saved.Secret, saved.Token = "", "", "", "", ""
			saved.Schedule, saved.Time, saved.Timezone = m.Backup.Draft.Schedule, m.Backup.Draft.Time, m.Backup.Draft.Timezone
			m.Backup.Draft = saved
			o := m.Backup.Commit()
			report(c, o, backupMessages)
			d.editing = o != kit.Done
		}, m.BackupActions != nil && m.Backup.Pending)
	}).Width(min(600, width(c)-48)).Label(title)
}

// archivesDialog is the archived sessions, each unarchived or deleted.
func archivesDialog(c *ui.Context, m *Model) {
	d := &m.backups
	if !d.archives {
		return
	}
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	title := t("backup_archived_sessions")
	kit.Dialog(c.Key("archives"), &d.archives, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		if len(m.Archived) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Archive, Title: t("backup_no_archived"), Card: true})
		} else {
			ui.Scroll(c.Key("list")).Grow(1).Shrink(1).Dividers(1, p.Border).Children(func() {
				for _, a := range m.Archived {
					// A session that never ran has no date.
					at := ""
					if !a.At.IsZero() {
						at = a.At.Format(time.DateOnly)
					}
					kit.Entry(c, a.ID, func() { kit.Record(c, a.Title, at) }, nil, func() {
						kit.TextActionBusy(c.Key("unarchive"), t("backup_unarchive"), m.Backup.Pending, m.Busy[BusyBackup+"unarchive/"+a.ID]).OnClick(func() {
							if x := m.BackupActions; x != nil {
								x.Unarchive(a.ID)
								return
							}
							notice(c)
						})
						kit.TextActionBusy(c.Key("delete"), t("backup_delete"), m.Backup.Pending, m.Busy[BusyBackup+"delete/"+a.ID]).TextColor(p.Danger).OnClick(func() {
							if m.BackupActions != nil {
								d.removed, d.removedTitle, d.removing = a.ID, a.Title, true
								return
							}
							notice(c)
						})
					})
				}
			})
		}
		ui.Row(c.Key("footer")).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("close"), t("close")).OnClick(func() { d.archives = false })
		})
	}).Width(min(560, width(c)-48)).Label(title)
}
