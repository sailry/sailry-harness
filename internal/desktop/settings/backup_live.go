package settings

import (
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// BackupRecord is one stored backup and the copies it can be restored from.
type BackupRecord struct {
	ID            string
	Created       time.Time
	Size          uint64
	Local, Online bool
}

// BackupRestore is a restore's source and form: a local archive File, or
// the stored backup ID from its local or Online copy; Target is the new
// profile's path on the host.
type BackupRestore struct {
	File               string
	ID                 string
	Online             bool
	Target, Passphrase string
}

// BackupActions are the live page's actions and what the Node listed.
// Their outcomes are toasts; done reports whether a dialog may close.
type BackupActions struct {
	// Host names the Node a restore writes to.
	Host string
	// Save saves the storage dialog's draft with its typed secrets. The
	// schedule's draft is saved as it changes.
	Save         func(target BackupTarget, done func(bool))
	ChooseFolder func(selected func(string))
	Export, Now  func()
	// Import chooses a local archive.
	Import func(selected func(string))
	// List reads the stored backups into Records.
	List              func()
	Restore           func(restore BackupRestore, done func(bool))
	Unarchive, Delete func(id string)
	ArchiveAll        func()
	DeleteAll         func()

	Records          []BackupRecord
	Partial, Listing bool
}

// CloseBackupDialogs drops the page's dialogs and their drafts, typed
// secrets included, when the page turns to another host.
func (m *Model) CloseBackupDialogs() { m.backups = backupDialogs{} }

// BackupEditing reports whether a backup dialog holds a draft.
func (m *Model) BackupEditing() bool { return m.backups.editing || m.backups.restoring }

// restoreProblem validates the restore form: the i18n key of the problem, or "".
func (r BackupRestore) restoreProblem(passphraseSet bool) string {
	switch {
	case strings.TrimSpace(r.Target) == "":
		return "backup_invalid_target"
	case r.Passphrase == "" && !passphraseSet:
		return "backup_passphrase_required"
	}
	return ""
}

// openRestore opens the restore form on a source.
func (d *backupDialogs) openRestore(source BackupRestore) {
	source.Target, source.Passphrase = "", ""
	d.restore, d.restoring = source, true
}

// recordsDialog lists the storage's backups; each restores from the copy chosen.
func recordsDialog(c *ui.Context, m *Model) {
	d, a := &m.backups, m.BackupActions
	if !d.records || a == nil {
		return
	}
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	title := t("backup_restore_storage")
	kit.Dialog(c.Key("records"), &d.records, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		switch {
		case len(a.Records) != 0:
			ui.Scroll(c.Key("list")).Grow(1).Shrink(1).Dividers(1, p.Border).Children(func() {
				for _, record := range a.Records {
					kit.Entry(c, record.ID, func() {
						kit.Record(c, record.Created.Local().Format("2006-01-02 15:04"), fmt.Sprintf("%.1f MiB", float64(record.Size)/(1024*1024)))
					}, nil, func() {
						if record.Local {
							kit.TextAction(c.Key("local"), t("backup_local"), m.Backup.Pending).OnClick(func() { d.openRestore(BackupRestore{ID: record.ID}) })
						}
						if record.Online {
							kit.TextAction(c.Key("online"), t("backup_online_enabled"), m.Backup.Pending).OnClick(func() { d.openRestore(BackupRestore{ID: record.ID, Online: true}) })
						}
					})
				}
			})
		case a.Listing:
			ui.Row(c.Key("loading")).Justify(ui.Center).Padding(24).Children(func() { ui.Spinner(c) })
		default:
			kit.Result(c, kit.ResultProps{Icon: icons.Archive, Title: t("backup_empty"), Card: true})
		}
		if a.Partial {
			ui.Text(c, t("backup_partial")).FontSize(14).TextColor(p.MutedForeground)
		}
		ui.Row(c.Key("footer")).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("close"), t("close")).OnClick(func() { d.records = false })
		})
	}).Width(min(640, width(c)-48)).Label(title)
}

// restoreDialog asks for the passphrase and the new profile's path. The
// host and the consequence are named once; a failed restore keeps the form.
func restoreDialog(c *ui.Context, m *Model) {
	d, a := &m.backups, m.BackupActions
	if !d.restoring || a == nil {
		return
	}
	p := theme.Of(c)
	t := func(k string) string { return i18n.T(c, k) }
	title := t("backup_restore")
	x := &d.restore
	kit.Dialog(c.Key("restore"), &d.restoring, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		ui.Column(c.Key("body")).Gap(16).Children(func() {
			if a.Host != "" {
				ui.Text(c, a.Host).FontSize(14).TextColor(p.MutedForeground)
			}
			ui.Text(c, t("backup_restore_consequence")).FontSize(14)
			label := t("backup_passphrase")
			kit.Field(c, "passphrase", label, "", func() {
				in := ui.TextInput(c.Key("input"), &x.Passphrase).Label(label).Password().FillWidth().Disabled(m.Backup.Pending)
				if m.Backup.Saved.PassphraseSet {
					in.Placeholder(t("backup_keep_secret"))
				}
			})
			target := t("backup_target")
			kit.Field(c, "target", target, "", func() {
				ui.TextInput(c.Key("input"), &x.Target).Label(target).FillWidth().Disabled(m.Backup.Pending)
			})
		})
		footer(c, title, func() { d.restoring = false }, func() {
			if m.Backup.Pending {
				return
			}
			if problem := x.restoreProblem(m.Backup.Saved.PassphraseSet); problem != "" {
				kit.Toast(c, kit.Failed, t(problem))
				return
			}
			request := *x
			request.Target = strings.TrimSpace(request.Target)
			a.Restore(request, func(ok bool) {
				if ok {
					d.restoring, d.records, d.restore = false, false, BackupRestore{}
				}
			})
		})
	}).Width(min(560, width(c)-48)).Label(title)
}
