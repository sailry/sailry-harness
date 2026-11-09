package settings

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

var backupMessages = Messages{"backup_saved", "backup_failed", "backup_conflict", "backup_unknown"}

// backupDialogs are the Backup page's dialogs: the storage editor with
// its draft, the archived sessions and deleting every session.
type backupDialogs struct {
	editing, archives, deleting bool
	draft                       BackupTarget
}

// Check validates the storage draft (backup/editor.rs save): the i18n
// key of the problem, or "".
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

// backup is backup/view.rs: the storage and the schedule, exporting,
// backing up and restoring, and the sessions' archive and deletion.
// Exports, backups and restores read and write archives, and the
// sessions are the user's: the preview stops at the notice for them.
func backup(c *ui.Context, m *Model) {
	b := &m.Backup
	d := &m.backups
	t := func(k string) string { return i18n.T(c, k) }
	target := b.Saved.PassphraseSet && (b.Saved.Tab == 2 && b.Saved.Directory != "" || b.Saved.Tab < 2 && b.Saved.Endpoint != "")
	entry := func(key, title, action string, run func()) {
		kit.Entry(c, key, func() { ui.Text(c, title).FontSize(14) }, nil, func() {
			kit.TextAction(c.Key("action"), action, b.Pending).OnClick(run)
		})
	}
	configure := func() {
		d.draft = b.Saved
		d.draft.Passphrase, d.draft.Confirm, d.draft.Identity, d.draft.Secret, d.draft.Token = "", "", "", "", ""
		d.editing = true
	}
	kit.Group(c, "backup", "", "", nil, func() {
		kit.Card(c, func() {
			entry("storage", t("backup_destination"), t("backup_configure"), configure)
			if kit.SwitchRow(c, "schedule", t("backup_schedule_enabled"), "", &b.Draft.Schedule) {
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
			export := t("backup_configure")
			if b.Saved.PassphraseSet {
				export = t("backup_export")
			}
			entry("export", t("backup_export_data"), export, func() {
				if !b.Saved.PassphraseSet {
					configure()
					return
				}
				notice(c)
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
				notice(c)
			})
			entry("import", t("backup_import"), t("backup_restore"), func() { notice(c) })
			kit.Entry(c, "storage-restore", func() { ui.Text(c, t("backup_restore_storage")).FontSize(14) }, nil, func() {
				kit.TextAction(c.Key("action"), t("backup_restore"), b.Pending || !target).OnClick(func() { notice(c) })
			})
			entry("archives", t("backup_archived_sessions"), t("backup_manage"), func() { d.archives = true })
			kit.Entry(c, "archive-all", func() { ui.Text(c, t("backup_archive_all")).FontSize(14) }, nil, func() {
				kit.TextAction(c.Key("action"), t("backup_archive_all_action"), b.Pending || m.Sessions == 0).OnClick(func() { notice(c) })
			})
			kit.Entry(c, "delete-all", func() { ui.Text(c, t("backup_delete_all")).FontSize(14) }, func() {
				kit.Danger(c.Key("action"), t("backup_delete_all_action")).Height(28).Disabled(b.Pending).OnClick(func() { d.deleting = true })
			}, nil)
		})
	})
	storageDialog(c, m)
	archivesDialog(c, m)
	if d.deleting {
		// Destructive: named and its consequence said once.
		if ui.AlertDialog(c.Key("delete-all"), &d.deleting, t("backup_delete_all")+"?", t("backup_delete_all_consequence"),
			t("settings_cancel"), t("backup_delete_all_action")) == 1 {
			notice(c)
		}
	}
}

// storageDialog is backup/editor.rs: WebDAV, S3 or a local folder, their
// fields, and the encryption passphrase. Saved secrets are never shown:
// their inputs are empty with a placeholder keeping them.
func storageDialog(c *ui.Context, m *Model) {
	d := &m.backups
	if !d.editing {
		return
	}
	t := func(k string) string { return i18n.T(c, k) }
	x := &d.draft
	title := t("backup_destination")
	kit.Dialog(c.Key("storage"), &d.editing, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(16).Children(func() {
			ui.Segmented(c.Key("tabs"), &x.Tab, t("backup_webdav"), t("backup_s3"), t("backup_local")).Label(title).FillWidth()
			input := func(key string, value *string, secret, kept bool) {
				label := t(key)
				kit.Field(c, key, label, "", func() {
					in := ui.TextInput(c.Key("input"), value).Label(label).FillWidth()
					if secret {
						in.Password()
					}
					if kept {
						in.Placeholder(t("backup_keep_secret"))
					}
				})
			}
			if x.Tab == 2 {
				label := t("backup_local_directory")
				kit.Field(c, "directory", label, "", func() {
					ui.Row(c).Gap(8).Children(func() {
						ui.TextInput(c.Key("input"), &x.Directory).Label(label).ReadOnly(true).Grow(1)
						// The folder picker browses the host's disk.
						kit.Secondary(c.Key("choose"), t("backup_choose_directory")).OnClick(func() { notice(c) })
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
				kit.TextAction(c.Key("clear"), t("backup_clear_credentials"), false).AlignSelf(ui.Start).OnClick(func() {
					x.CredentialsSet = false
				})
			}
		})
		footer(c, t("settings_save"), func() { d.editing = false }, func() {
			if problem := x.Check(); problem != "" {
				kit.Toast(c, kit.Failed, t(problem))
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
		})
	}).Width(min(600, width(c)-48)).Label(title)
}

// archivesDialog is backup/sessions.rs: the archived sessions, each
// unarchived or deleted.
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
			kit.EmptyGroup(c, icons.Archive, t("backup_no_archived"))
		} else {
			ui.Scroll(c.Key("list")).Grow(1).Shrink(1).Dividers(1, p.Border).Children(func() {
				for _, a := range m.Archived {
					kit.Entry(c, a.ID, func() { kit.Record(c, a.Title, a.At.Format(time.DateOnly)) }, nil, func() {
						kit.TextAction(c.Key("unarchive"), t("backup_unarchive"), false).OnClick(func() { notice(c) })
						kit.TextAction(c.Key("delete"), t("backup_delete"), false).TextColor(p.Danger).OnClick(func() { notice(c) })
					})
				}
			})
		}
		ui.Row(c.Key("footer")).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("close"), t("close")).OnClick(func() { d.archives = false })
		})
	}).Width(min(560, width(c)-48)).Label(title)
}
