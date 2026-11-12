package panel

import (
	"slices"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// allChanges is the item of the diff of every change (Diff, the git
// plugin's combined document); no path starts with ":".
const allChanges = ":changes"

// gitDetails is the Git component's side column, as the git plugin's
// view.js details lays it out: the branch, the actions menu and Refresh;
// Changes | History; then the changes' toolbar, their list and the commit
// box, or the history. open shows a change's diff, source a file in
// Files.
func (pr *Project) gitDetails(c *ui.Context, own bool, open func(path string)) {
	p := theme.Of(c)
	v := &pr.Git
	g := pr.menus(c)
	ui.Row(c.Key("git-header")).Height(barHeight).Shrink(0).PaddingX(8).Gap(4).BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
		branch := pr.Data.Repo.Branch
		if branch == "" {
			branch = i18n.T(c, "git_branches")
		}
		b := kit.Ghost(c.Key("branch"), branch, false).Height(28).PaddingX(8).Gap(6).Shrink(1).MinWidth(0).Justify(ui.Start).Disabled(!pr.gitWritable())
		b.Children(func() {
			ui.Icon(c, icons.Network).FontSize(14).Shrink(0)
			ui.Text(c, branch).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			ui.Icon(c, icons.ChevronDown).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
		}).OnClick(func() { g.choose("git_checkout", g.checkout(), "git_checkout").run(c) })
		ui.Spacer(c)
		kit.MenuButton(c.Key("actions"), "git-actions-"+pr.ID, func(b ui.Element) {
			b.Disabled(!pr.gitWritable())
			kit.IconLook(c, b, icons.Ellipsis, i18n.T(c, "git_actions"), "", 28, false)
		}, func(m *kit.Menu) { fill(c, m, g.root()) }, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
		if pr.gitBusy() {
			kit.BusyAction(c.Key("refresh"), i18n.T(c, "git_refresh"))
		} else {
			kit.IconButton(c.Key("refresh"), icons.RotateCw, i18n.T(c, "git_refresh"), 28, false).OnClick(func() { pr.refresh(c) })
		}
	})
	if pr.gitEmpty(c, true) {
		pr.gitDialogs(c)
		return
	}
	pending := append([]PendingOperation(nil), v.switchPending...)
	if pr.Actions != nil && pr.Actions.PendingGit != nil {
		pending = append(pending, pr.Actions.PendingGit()...)
	}
	if len(pending) > 0 {
		ui.Row(c.Key("git-switch-pending")).Padding(8).Gap(8).Children(func() {
			for index, operation := range pending {
				kit.Secondary(c.Key(index), i18n.T(c, operation.Label)).OnClick(func() {
					if operation.Run != nil {
						operation.Run(c)
					}
				})
			}
		})
	}
	ui.Column(c.Key("views")).Padding(8, 8, 4, 8).Shrink(0).Children(func() {
		kit.Segmented(c, "git-views", &v.view, []string{i18n.T(c, "git_view_changes"), i18n.T(c, "git_view_history")},
			kit.SegmentedProps{Block: true})
	})
	if v.view == 1 {
		pr.history(c, own)
	} else {
		pr.changesToolbar(c, open)
		pr.changeList(c, own, open)
		pr.commitBox(c)
	}
	pr.gitDialogs(c)
}

// changesToolbar is Diff, the changes' total +N −M, and Stage all or
// Unstage all with its menu.
func (pr *Project) changesToolbar(c *ui.Context, open func(path string)) {
	p := theme.Of(c)
	entries := pr.Data.Changes
	add, del := 0, 0
	for _, e := range entries {
		add, del = add+e.Added, del+e.Removed
	}
	unstaged := len(pr.stagePaths(entries, false)) > 0
	staged := len(indexPaths(entries, "unstage")) > 0
	key := "git_unstage"
	if unstaged {
		key = "git_stage"
	}
	ui.Row(c.Key("git-toolbar")).PaddingX(8).PaddingY(4).Gap(4).Shrink(0).Children(func() {
		label := i18n.T(c, "git_diff")
		b := kit.Ghost(c.Key("diff"), label, false).Height(28).PaddingX(8).Disabled(len(entries) == 0)
		kit.Tooltip(c, b, i18n.T(c, "git_view_diff"))
		b.Children(func() { ui.Text(c, label).FontSize(13) }).OnClick(func() { open(allChanges) })
		stats(c, p, add, del)
		ui.Spacer(c)
		g := pr.menus(c)
		kit.SplitButton(c.Key("stage"), "git-stage-"+pr.ID, i18n.T(c, key), i18n.T(c, "more"),
			kit.SplitProps{Small: true, Disabled: !staged && !unstaged}, func(m *kit.Menu) { fill(c, m, g.changes()[:2]) }).
			OnClick(func() {
				kind := "git_unstage_all"
				if unstaged {
					kind = "git_stage_all"
				}
				pr.gitAction(c, GitAction{Kind: kind})
			})
	})
}

// stagePaths are the paths Stage sends for entries. A conflicted file is
// staged only as the file of its own row, which records its resolution; a
// folder, a group and Stage all leave each conflict to be resolved by itself.
func (pr *Project) stagePaths(entries []preview.FileChange, file bool) []string {
	var out []string
	for _, relative := range indexPaths(entries, "stage") {
		if file || !pr.gitConflicted(relative) {
			out = append(out, relative)
		}
	}
	return out
}

// stats is a change's +N −M, each only when not 0.
func stats(c *ui.Context, p *theme.Palette, add, del int) {
	if add > 0 {
		ui.Textf(c, "+%d", add).FontSize(12).Shrink(0).TextColor(p.Success)
	}
	if del > 0 {
		ui.Textf(c, "−%d", del).FontSize(12).Shrink(0).TextColor(p.Danger)
	}
}

// statusColor is the color of a change's letter.
func statusColor(p *theme.Palette, st string) ui.Color {
	switch st {
	case "A":
		return p.Success
	case "D":
		return p.Danger
	}
	return p.Warning
}

// changeList is the working tree's changes as a tree (view.js changes):
// the groups as headings with their count, folders when hierarchical, a
// change's letter, name, folder when flat, +N −M and stage box. A click
// moves the cursor there and opens a change's diff, the keys moving it
// open theirs; a group or folder opens and closes. The change shown is
// selected. A row's right-click menu acts on every change in it.
func (pr *Project) changeList(c *ui.Context, own bool, open func(path string)) {
	p := theme.Of(c)
	v := &pr.Git
	if len(pr.Data.Changes) == 0 {
		kit.Result(c, kit.ResultProps{Icon: icons.Network, Title: i18n.T(c, "git_no_changes")}).Grow(1).Justify(ui.Center)
		return
	}
	g := pr.menus(c)
	rows := changeRows(pr.Data.Changes, v.nav, func(key string, n int) string {
		return i18n.T(c, key) + " · " + strconv.Itoa(n)
	})
	var shown *preview.FileChange
	for i := range pr.Data.Changes {
		if pr.Data.Changes[i].Path == v.change {
			shown = &pr.Data.Changes[i]
		}
	}
	selected := selectedID(shown, v.nav.grouping, v.scope)
	skip := 1
	if v.nav.grouping == "none" {
		skip = 0
	}
	ui.Scroll(c.Key("files")).TrackScroll(scroll(&v.list, own)).Grow(1).Padding(0, 8, 8, 8).Children(func() {
		tree := ui.TreeBase(c)
		tree.Root.Gap(0).Children(func() {
			var node func(r *changeRow)
			node = func(r *changeRow) {
				c.Key(r.id)
				var folder *bool
				if r.entry == nil {
					if v.expanded[r.id] == nil {
						o := true
						v.expanded[r.id] = &o
					}
					folder = v.expanded[r.id]
				}
				item := tree.Item(folder)
				depth := max(item.Depth()-skip, 0)
				row := item.Row.Height(rowHeight).PaddingStart(8 + 14*float32(depth)).PaddingEnd(8).Gap(8).Radius(p.Radius).
					Cursor(ui.CursorPointer).Label(r.label).TextColor(p.MutedForeground)
				group := r.entries != nil
				switch {
				case r.id == selected && !group:
					row.Background(p.Hover).TextColor(p.HoverForeground).AccessSelected(true)
				case row.Hovered():
					row.TextColor(p.Foreground)
				}
				pick := func() {
					v.current = r.id
					if r.entry != nil {
						open(r.entry.Path)
						v.scope = r.scope()
					}
				}
				if row.Clicked() {
					pick()
					if folder != nil && row.ClickModifiers() == 0 {
						*folder = !*folder
					}
				} else if row.FocusVisible() && v.current != r.id {
					pick()
				}
				entries := r.descendants()
				row.ContextMenu(func(m *ui.Menu) {
					directory := ""
					if parts := strings.SplitN(r.id, "/", 3); len(parts) == 3 && parts[0] == "dir" {
						directory = parts[2]
					}
					fillNative(c, m, g.files(entries, r.entry == nil, directory))
				})
				if r.entry != nil {
					kit.Tooltip(c, row, r.entry.Path)
				}
				row.Children(func() {
					switch {
					case r.entry != nil:
						k := kind(*r.entry, r.scope())
						ui.Text(c, k).FontSize(12).FontWeight(600).Width(12).TextAlign(ui.Center).Shrink(0).TextColor(statusColor(p, k))
					case !group:
						ui.Icon(c, icons.Folder).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
					}
					ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(8).Children(func() {
						ui.Text(c, r.label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
						if r.entry != nil && !v.nav.hierarchical {
							if dir := parentDir(r.entry.Path); dir != "" {
								ui.Text(c, dir).FontSize(14).SingleLine().Shrink(1).MinWidth(0).MaxWidthPercent(40).TextColor(p.MutedForeground)
							}
						}
					})
					if r.entry != nil {
						stats(c, p, r.entry.Added, r.entry.Removed)
					}
					if group {
						arrow := icons.ChevronRight
						if *folder {
							arrow = icons.ChevronDown
						}
						ui.Icon(c, arrow).FontSize(12).Shrink(0)
					}
					checked, mixed := checkState(entries, r.scope())
					state, label := ui.CheckOff, "git_stage"
					switch {
					case checked:
						state, label = ui.CheckOn, "git_unstage"
					case mixed:
						state = ui.CheckMixed
					}
					kind := "git_stage"
					paths := pr.stagePaths(entries, r.entry != nil)
					if checked {
						kind, paths = "git_unstage", indexPaths(entries, "unstage")
					}
					box := ui.CheckboxState(c.Key("check"), &state, "").Label(i18n.T(c, label)).Shrink(0).Disabled(!pr.gitWritable() || len(paths) == 0)
					if box.Changed() {
						pr.gitAction(c, GitAction{Kind: kind, Paths: paths})
					}
				})
				item.Group(func() {
					for _, k := range r.children {
						node(k)
					}
				})
			}
			for _, r := range rows {
				node(r)
			}
		})
	})
}

// showChange opens a change's diff in scope.
func (pr *Project) showChange(path, scope string) {
	pr.Git.Show(path)
	pr.Git.scope = scope
	pr.readDiff(path, scope)
}

// source opens a changed file in Files, by its path in the project's tree.
func (pr *Project) source(path string) {
	if pr.Actions != nil {
		pr.FileOpen(path)
		if pr.reveal != nil {
			pr.reveal(path)
		}
		return
	}
	if f := findFile(pr.Data.Files, pr.Data.Files.Name+"/"+path); f != nil {
		pr.Files.Show(f.Path)
		if pr.reveal != nil {
			pr.reveal(f.Path)
		}
	}
}

// commitBox is the commit message and Commit (view.js commit): the
// amend, sign-off and skip-hooks options when on, the message with its
// expand button, and Commit with its menu.
func (pr *Project) commitBox(c *ui.Context) {
	p := theme.Of(c)
	v := &pr.Git
	g := pr.menus(c)
	ui.Column(c.Key("git-commit")).Padding(8).Gap(8).Shrink(0).BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Children(func() {
		if v.amend || v.signoff || v.skip {
			ui.Row(c.Key("options")).Gap(8).TextColor(p.MutedForeground).Children(func() {
				if v.amend {
					label := i18n.T(c, "git_amend")
					kit.Ghost(c.Key("amend"), label, false).Height(20).PaddingX(6).
						Children(func() { ui.Text(c, label).FontSize(12) }).OnClick(func() { pr.toggleAmend() })
				}
				if v.signoff {
					ui.Text(c, i18n.T(c, "git_signoff")).FontSize(12)
				}
				if v.skip {
					ui.Text(c, i18n.T(c, "git_skip_hooks")).FontSize(12)
				}
			})
		}
		h := float32(86)
		if v.tall {
			h = 220
		}
		ui.Row(c.Key("message-box")).PaddingEnd(24).Children(func() {
			label := i18n.T(c, "git_commit_message")
			ui.TextAreaBase(c.Key("message"), &v.commit).Placeholder(label).Label(label).FillWidth().Height(h).FontSize(14)
			kit.IconButton(c.Key("expand"), icons.Maximize, i18n.T(c, "git_expand_commit"), 20, v.tall).
				Absolute().Top(0).Right(0).OnClick(func() { v.tall = !v.tall })
		})
		head := pr.gitHasHead()
		can := pr.gitCanCommit() && strings.TrimSpace(v.commit) != "" && (v.amend && head || !v.amend && len(pr.Data.Changes) > 0)
		kit.SplitButton(c.Key("commit"), "git-commit-"+pr.ID, i18n.T(c, "git_commit"), i18n.T(c, "more"),
			kit.SplitProps{Icon: icons.Check, Disabled: !can, Fill: true}, func(m *kit.Menu) { fill(c, m, g.commitSplit()) }).
			OnClick(func() { pr.commit(c, "auto", v.amend, v.signoff, v.after) })
	})
}

// toggleAmend starts amending the last commit, its message replacing the
// draft, or stops, the draft back.
func (pr *Project) toggleAmend() {
	v := &pr.Git
	if !pr.gitHasHead() {
		return
	}
	v.amend = !v.amend
	if v.amend {
		v.draft = v.commit
		if pr.Actions != nil {
			v.commit = v.live.HeadMessage
		} else {
			v.commit = pr.Data.Repo.Commits[0].Message
		}
	} else {
		v.commit, v.draft = v.draft, ""
	}
}

// commit is actions.js commit: it sets the options; starting to amend
// fills the message, and without a message there is nothing to commit
// yet. Committing would write the repository.
func (pr *Project) commit(c *ui.Context, scope string, amend, signoff bool, after string) {
	if !pr.gitCanCommit() {
		return
	}
	v := &pr.Git
	entering := amend && !v.amend
	if entering {
		pr.toggleAmend()
	}
	v.amend, v.signoff, v.after = amend, signoff, after
	if entering || strings.TrimSpace(v.commit) == "" {
		return
	}
	pr.gitAction(c, GitAction{Kind: "git_commit", Message: v.commit, Scope: scope, Amend: amend, Signoff: signoff, SkipHooks: v.skip, After: after})
}

// gitKeys are the Git component's shortcuts (main.js): Cmd+Enter commits,
// Cmd+Shift+Enter amends, Ctrl+G Ctrl+G fetches, Ctrl+G Down pulls,
// Ctrl+G Shift+Down pulls with rebase and Cmd+Shift+K pushes.
func (pr *Project) gitKeys(c *ui.Context, e ui.Element) {
	v := &pr.Git
	// Asking for a key applies the frame's input, the observer below with
	// it, so the chord's second key is read after one. Down is asked for
	// only while the chord waits for it: otherwise it is whoever's it was.
	armed := v.chord
	prefix := e.Shortcut(ui.Ctrl, ui.KeyG)
	second := v.second
	v.second = false
	pull := (armed || second) && e.Shortcut(0, ui.KeyDown)
	rebase := (armed || second) && e.Shortcut(ui.Shift, ui.KeyDown)
	switch {
	case e.Shortcut(ui.Cmd, ui.KeyEnter):
		pr.commit(c, "auto", v.amend, v.signoff, v.after)
	case e.Shortcut(ui.Cmd|ui.Shift, ui.KeyEnter):
		pr.toggleAmend()
	case e.Shortcut(ui.Cmd|ui.Shift, ui.KeyK):
		pr.gitAction(c, GitAction{Kind: "git_push"})
	case second && (prefix || pull || rebase):
		kind := "git_fetch_all_prune"
		if pull {
			kind = "git_pull"
		} else if rebase {
			kind = "git_pull_rebase"
		}
		pr.gitAction(c, GitAction{Kind: kind})
	case prefix:
		v.chord = true
	}
	// The key after Ctrl+G completes the chord or drops it, as the
	// reference's key sequence: a Down pressed later, or one a list or a
	// field took, does not pull.
	e.ObserveInput(func(ev ui.InputEvent) {
		// A modifier on its way to the second key is no key yet.
		if v.chord && ev.Kind == ui.InputKeyDown && ev.Key != ui.KeyUnknown && !ev.Repeat {
			v.chord, v.second = false, true
		}
	})
}

// commitBranch is the local branch at a commit, the current one first.
func commitBranch(cm preview.Commit, current string) string {
	if slices.Contains(cm.Refs, "heads/"+current) {
		return current
	}
	for _, r := range cm.Refs {
		if name, ok := strings.CutPrefix(r, "heads/"); ok {
			return name
		}
	}
	return ""
}

// gitDialogs are the picker and the confirmation, when showing.
func (pr *Project) gitDialogs(c *ui.Context) {
	v := &pr.Git
	if pk := &v.picker; pk.open {
		q := strings.ToLower(strings.TrimSpace(pk.query))
		var items []action
		var rows []kit.CommandItem
		for _, it := range pk.items {
			if q == "" || strings.Contains(strings.ToLower(it.label), q) {
				items = append(items, it)
				rows = append(rows, kit.CommandItem{Label: it.label, Detail: it.detail, Icon: it.icon, Group: it.group,
					Checked: it.checked, Disabled: it.disabled})
			}
		}
		kit.Command(c, "git-picker", &pk.open, &pk.query, &pk.sel, rows, kit.CommandProps{
			Placeholder: pk.title, Empty: i18n.T(c, "git_no_matches"),
		}, func(i int) {
			it := items[i]
			*pk = picker{}
			it.run(c)
		})
	}
	if cf := &v.confirm; cf.open {
		// Destructive: the target named and the consequence said once.
		if kit.AlertDialog(c.Key("git-confirm"), &cf.open, cf.title, cf.message+"\n"+cf.target,
			i18n.T(c, "settings_cancel"), cf.title) == 1 {
			if cf.run != nil {
				cf.run(c)
			} else {
				notice(c)
			}
		}
	}
}

// combined is every change's diff (view.js combined): each file under a
// heading of its name, folder and +N −M, which closes it, with Undo.
func (pr *Project) combined(c *ui.Context) {
	p := theme.Of(c)
	v := &pr.Git
	if len(pr.Data.Changes) == 0 {
		kit.Result(c, kit.ResultProps{Icon: icons.Network, Title: i18n.T(c, "git_no_changes")}).Grow(1).Justify(ui.Center)
		return
	}
	add, del := 0, 0
	ui.Scroll(c.Key("git-combined")).Grow(1).Padding(8).Gap(8).Children(func() {
		for _, ch := range pr.Data.Changes {
			pr.readDiff(ch.Path, "all")
			add, del = add+ch.Added, del+ch.Removed
			open := !v.folded[ch.Path]
			ui.Column(c.Key(ch.Path)).Shrink(0).Radius(p.Radius).Border(1, p.Border).Clip().Children(func() {
				head := ui.ButtonBase(c.Key("heading")).Label(ch.Path).Height(32).PaddingX(8).Gap(8).Cursor(ui.CursorPointer).
					Expanded(open)
				if head.Hovered() {
					head.Background(p.Hover)
				}
				head.Children(func() {
					arrow := icons.ChevronRight
					if open {
						arrow = icons.ChevronDown
					}
					ui.Icon(c, arrow).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
					ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(8).Children(func() {
						ui.Text(c, baseName(ch.Path)).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
						if dir := parentDir(ch.Path); dir != "" {
							ui.Text(c, dir).FontSize(14).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
						}
					})
					stats(c, p, ch.Added, ch.Removed)
					kit.IconButton(c.Key("undo"), icons.Undo, i18n.T(c, "turn_changes_undo"), 24, false).OnClick(func() {
						v.confirm = confirm{open: true, title: i18n.T(c, "turn_changes_undo"), message: i18n.T(c, "git_discard_all_confirm"), target: ch.Path, run: func(c *ui.Context) { pr.gitAction(c, GitAction{Kind: "git_discard_all", Paths: []string{ch.Path}}) }}
					})
				}).OnClick(func() { v.folded[ch.Path] = open })
				if open {
					old, new := ch.Texts()
					metadata := v.diffStates["all/"+ch.Path]
					if metadata.Binary {
						ui.Text(c, i18n.T(c, "git_diff_binary")).FontSize(14).Padding(8, 12).TextColor(p.MutedForeground)
					} else {
						kit.Diff(c, ui.Column(c.Key("diff")).BorderWidth(1, 0, 0, 0).BorderColor(p.Border), old, new,
							kit.DiffOptions{Language: editor.LanguageFor(ch.Path), Fit: true})
						if metadata.Partial {
							ui.Text(c, i18n.T(c, "git_diff_partial")).FontSize(12).Padding(8, 12).TextColor(p.MutedForeground)
						}
					}
				}
			})
		}
	})
	status(c, i18n.T(c, "git_all_changes"), func() { stats(c, p, add, del) })
}
