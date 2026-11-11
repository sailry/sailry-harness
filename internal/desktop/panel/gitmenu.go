package panel

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The Git component's menus as the git plugin's menus.js builds them:
// the actions menu (root), the commit button's (commitSplit), the stage
// button's (changes) and a change's right-click menu (files). An action
// that would change the repository or reach a remote shows the preview's
// notice and does nothing else; one that only changes the view (the
// list's layout, the commit options, a picker) works.

// action is an item of a Git menu or picker: a submenu (sub), a
// separator, or an item that run does; a picker row has a group, an
// icon and a detail.
type action struct {
	label         string
	disabled      bool
	checked       bool
	sep           bool
	sub           []action
	icon          *ui.SVG
	group, detail string
	run           func(c *ui.Context)
}

var separatorItem = action{sep: true}

// picker is a searchable list of choices (Sailry's Picker), as Checkout
// lists the branches.
type picker struct {
	open  bool
	title string
	query string
	sel   int
	items []action
}

// confirm is a destructive action's confirmation, naming its target.
type confirm struct {
	open    bool
	title   string // the action, which its button says
	message string // its consequence
	target  string
	run     func(*ui.Context)
}

// gitMenus builds the menus of a project's Git component.
type gitMenus struct {
	c  *ui.Context
	pr *Project
	v  *GitView
}

func (pr *Project) menus(c *ui.Context) gitMenus { return gitMenus{c, pr, &pr.Git} }

func (g gitMenus) t(key string, args ...any) string { return i18n.T(g.c, key, args...) }

func (g gitMenus) repo() *preview.Repository { return &g.pr.Data.Repo }

func (g gitMenus) entries() []preview.FileChange { return g.pr.Data.Changes }

// item is an action labeled key's text.
func (g gitMenus) item(key string, run func(c *ui.Context), enabled bool) action {
	return action{label: g.t(key), disabled: !enabled, run: run}
}

// run is an action on the repository, which the preview does not take.
func (g gitMenus) run(key string, enabled bool) action {
	return g.item(key, func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: key}) }, enabled)
}

// confirm asks before a destructive action: title names it, message
// says what it does, target what it acts on.
func (g gitMenus) confirm(key, message, target string, enabled bool, label string) action {
	a := g.item(key, func(*ui.Context) {
		action := GitAction{Kind: key, Target: target, Paths: strings.Split(target, "\n")}
		if key == "git_stash_drop_all" {
			action.Paths = append([]string(nil), g.v.stashIDs...)
		}
		g.v.confirm = confirm{open: true, title: g.t(key), message: g.t(message), target: target, run: func(c *ui.Context) {
			g.pr.gitAction(c, action)
		}}
	}, enabled)
	a.label = g.t(label)
	return a
}

// choose opens a picker of items, titled key's text.
func (g gitMenus) choose(key string, items []action, label string) action {
	title := key
	if key == "git_checkout" {
		title = "git_search_branches"
	}
	a := g.item(key, func(*ui.Context) { g.v.picker = picker{open: true, title: g.t(title), items: items} }, len(items) > 0)
	a.label = g.t(label)
	return a
}

func (g gitMenus) section(key string, children []action) action {
	return action{label: g.t(key), sub: children}
}

// remote are the remotes as choices, each doing do.
func (g gitMenus) remote(do func(name string) func(c *ui.Context)) []action {
	var out []action
	for _, r := range g.repo().Remotes {
		out = append(out, action{label: r.Name, detail: r.URL, run: do(r.Name)})
	}
	return out
}

func (g gitMenus) head() bool { return g.pr.gitHasHead() }

func (g gitMenus) staged() bool {
	return slices.ContainsFunc(g.entries(), func(e preview.FileChange) bool { return e.Staged != "" })
}

// branches are the branches an operation applies to, as choices.
func (g gitMenus) branches(op string) []action {
	var out []action
	for _, b := range g.repo().Branches {
		if op == "rename" && b.Remote || op == "delete" && (b.Remote || b.Current) || (op == "merge" || op == "rebase") && b.Current {
			continue
		}
		group := "git_local_branches"
		if b.Remote {
			group = "git_remote_branches"
		}
		detail := b.Tracking
		if detail == "" {
			detail = b.Commit[:min(7, len(b.Commit))]
		}
		out = append(out, action{label: b.Name, checked: b.Current, icon: icons.Network, group: g.t(group), detail: detail,
			run: g.branch(op, b)})
	}
	return out
}

// branch is a branch operation: copying its name works; switching to
// the current branch does nothing; deleting and merging ask first; the
// rest would change the repository.
func (g gitMenus) branch(op string, b preview.Branch) func(c *ui.Context) {
	return func(c *ui.Context) {
		switch {
		case op == "copy":
			c.WriteClipboard(b.Name)
		case op == "switch" && b.Current:
		case op == "delete":
			g.v.confirm = confirm{open: true, title: g.t("git_delete_branch"), message: g.t("git_delete_confirm"), target: b.Name, run: func(c *ui.Context) {
				g.pr.gitAction(c, GitAction{Kind: "branch_" + op, Target: b.Name, Secondary: b.Commit})
			}}
		case op == "merge":
			g.v.confirm = confirm{open: true, title: g.t("git_merge_branch"), message: g.t("git_merge_branch_confirm"), target: b.Name, run: func(c *ui.Context) {
				g.pr.gitAction(c, GitAction{Kind: "branch_" + op, Target: b.Name, Secondary: b.Commit})
			}}
		default:
			g.pr.gitAction(c, GitAction{Kind: "branch_" + op, Target: b.Name, Secondary: b.Commit})
		}
	}
}

func (g gitMenus) branchActions() []action {
	group := g.t("git_branch_actions")
	out := []action{{label: g.t("git_new_branch"), icon: icons.Plus, group: group, run: func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: "branch_create"}) }}}
	if len(g.repo().Branches) > 0 {
		out = append(out,
			action{label: g.t("git_branch_from_open"), icon: icons.Network, group: group, run: g.choose("git_branch_from", g.branches("create"), "git_branch_from").run},
			action{label: g.t("git_copy_branch"), icon: icons.Copy, group: group, run: g.choose("git_copy_branch", g.branches("copy"), "git_copy_branch").run})
	}
	return out
}

// checkout is the branch button's list: the local and remote branches,
// then the branch actions.
func (g gitMenus) checkout() []action { return append(g.branches("switch"), g.branchActions()...) }

// management is Manage branches: each branch opens the actions on it.
func (g gitMenus) management() []action {
	var out []action
	for _, b := range g.repo().Branches {
		var items []action
		for _, o := range []struct {
			key, op string
			ok      bool
		}{{"git_checkout", "switch", !b.Current}, {"git_copy_branch", "copy", true}, {"git_branch_from_open", "create", true},
			{"git_rename_branch", "rename", !b.Remote}, {"git_delete_branch", "delete", !b.Remote && !b.Current},
			{"git_merge_branch", "merge", !b.Current}, {"git_rebase", "rebase", !b.Current}} {
			if o.ok {
				items = append(items, action{label: g.t(o.key), run: g.branch(o.op, b)})
			}
		}
		if b.Remote {
			items = append(items, action{label: g.t("git_delete_remote_branch_open"), run: func(*ui.Context) {
				g.v.confirm = confirm{open: true, title: g.t("git_delete_remote_branch"), message: g.t("git_delete_confirm"), target: b.Name, run: func(c *ui.Context) {
					g.pr.gitAction(c, GitAction{Kind: "git_delete_remote_branch", Target: b.Name, Secondary: b.Commit})
				}}
			}})
		}
		items = append(items, action{label: g.t("worktree_create"), run: func(c *ui.Context) {
			g.pr.gitAction(c, GitAction{Kind: "worktree_create", Target: b.Name, Secondary: b.Commit})
		}})
		group := "git_local_branches"
		if b.Remote {
			group = "git_remote_branches"
		}
		detail := b.Tracking
		if detail == "" {
			detail = b.Commit[:min(7, len(b.Commit))]
		}
		out = append(out, action{label: b.Name, icon: icons.Network, checked: b.Current, group: g.t(group), detail: detail,
			run: func(*ui.Context) { g.v.picker = picker{open: true, title: b.Name, items: items} }})
	}
	return append(out, g.branchActions()...)
}

// push pushes to the upstream, or chooses a remote without one; force
// asks first.
func (g gitMenus) push(force bool) action {
	key := "git_push"
	if force {
		key = "git_force_push"
	}
	if up := g.repo().Upstream; up != "" {
		if force {
			return g.confirm(key, "git_force_push_confirm", up, g.head(), key)
		}
		return g.run(key, g.head())
	}
	return g.choose(key, g.remote(func(name string) func(c *ui.Context) {
		if force {
			return func(*ui.Context) {
				g.v.confirm = confirm{open: true, title: g.t(key), message: g.t("git_force_push_confirm"), target: name, run: func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: key, Remote: name, Force: force}) }}
			}
		}
		return func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: key, Remote: name, Force: force}) }
	}), key)
}

// viewOptions are the list's layout, sort and grouping, each checked
// while chosen.
func (g gitMenus) viewOptions() []action {
	nav := &g.v.nav
	var out []action
	for i, o := range []struct {
		key, field, value string
	}{{"git_layout_list", "hierarchical", "false"}, {"git_layout_tree", "hierarchical", "true"},
		{"git_sort_path", "sort", "path"}, {"git_sort_name", "sort", "name"}, {"git_sort_status", "sort", "status"},
		{"git_group_none", "grouping", "none"}, {"git_group_tracked", "grouping", "tracked"}, {"git_group_staged", "grouping", "staged"}} {
		if i == 2 || i == 5 {
			out = append(out, separatorItem)
		}
		field := map[string]*string{"sort": &nav.sort, "grouping": &nav.grouping}[o.field]
		var on bool
		if field == nil {
			on = nav.hierarchical == (o.value == "true")
		} else {
			on = *field == o.value
		}
		a := g.item(o.key, func(*ui.Context) {
			if field == nil {
				nav.hierarchical = o.value == "true"
			} else {
				*field = o.value
			}
		}, true)
		a.checked = on
		out = append(out, a)
	}
	return out
}

// commit commits as the Commit menus' items do (actions.js commit).
func (g gitMenus) commit(scope string, amend, signoff bool, after string) func(c *ui.Context) {
	return func(c *ui.Context) { g.pr.commit(c, scope, amend, signoff, after) }
}

func (g gitMenus) commits() []action {
	var out []action
	entries := len(g.entries()) > 0
	for _, set := range []struct {
		amend, signoff bool
		keys           [3]string
	}{{false, false, [3]string{"git_commit", "git_commit_staged", "git_commit_all"}},
		{true, false, [3]string{"git_commit_amend", "git_commit_staged_amend", "git_commit_all_amend"}},
		{false, true, [3]string{"git_commit_signoff", "git_commit_staged_signoff", "git_commit_all_signoff"}}} {
		if set.amend || set.signoff {
			out = append(out, separatorItem)
		}
		for i, key := range set.keys {
			ok := entries
			switch {
			case set.amend:
				ok = g.head()
			case i == 1:
				ok = g.staged()
			}
			out = append(out, g.item(key, g.commit([3]string{"auto", "staged", "all"}[i], set.amend, set.signoff, "none"), ok && g.pr.gitCanCommit()))
		}
		if !set.amend && !set.signoff {
			out = append(out, g.confirm("git_undo_commit", "git_undo_confirm", g.headSummary(), g.head(), "git_undo_commit"),
				g.run("git_continue", g.v.operation && len(g.v.live.Conflicts) == 0), g.run("git_abort", g.v.operation))
		}
	}
	skip := g.item("git_skip_hooks", func(*ui.Context) { g.v.skip = !g.v.skip }, true)
	skip.checked = g.v.skip
	return append(out, separatorItem, skip)
}

// headSummary is the last commit's first line.
func (g gitMenus) headSummary() string {
	if g.pr.Actions != nil {
		summary, _, _ := strings.Cut(g.v.live.HeadMessage, "\n")
		return summary
	}
	if !g.head() {
		return ""
	}
	s, _, _ := strings.Cut(g.repo().Commits[0].Message, "\n")
	return s
}

// commitSplit is the commit button's menu.
func (g gitMenus) commitSplit() []action {
	var out []action
	for i, o := range []struct {
		key   string
		amend bool
		after string
	}{{"git_commit", false, "none"}, {"git_commit_amend", true, "none"}, {"git_commit_push", false, "push"}, {"git_commit_sync", false, "sync"}} {
		if i == 2 {
			out = append(out, separatorItem)
		}
		out = append(out, g.item(o.key, g.commit("auto", o.amend, false, o.after), !o.amend || g.head()))
	}
	return out
}

// changes stages, unstages or discards every change.
func (g gitMenus) changes() []action {
	var paths []string
	for _, e := range g.entries() {
		paths = append(paths, e.Path)
	}
	return []action{
		g.run("git_stage_all", len(indexPaths(g.entries(), "stage")) > 0),
		g.run("git_unstage_all", len(indexPaths(g.entries(), "unstage")) > 0),
		g.confirm("git_discard_all", "git_discard_all_confirm", strings.Join(paths, "\n"), len(paths) > 0, "git_discard_all"),
	}
}

func (g gitMenus) network() []action {
	repo := g.repo()
	upstream, remotes := repo.Upstream != "", len(repo.Remotes) > 0
	var pulls []action
	for _, b := range repo.Branches {
		if !b.Remote {
			continue
		}
		for _, r := range repo.Remotes {
			if strings.HasPrefix(b.Name, r.Name+"/") {
				pulls = append(pulls, action{label: b.Name, run: func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: "git_pull_from", Target: b.Name}) }})
				break
			}
		}
	}
	out := []action{g.run("git_sync", upstream), g.run("git_sync_rebase", upstream), separatorItem,
		g.run("git_pull", upstream), g.run("git_pull_rebase", upstream), g.choose("git_pull_from", pulls, "git_pull_from_open"), separatorItem,
		g.push(false), g.choose("git_push_to", g.remote(func(name string) func(*ui.Context) {
			return func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: "git_push_to", Remote: name}) }
		}), "git_push_to"), g.push(true), separatorItem}
	for _, key := range []string{"git_fetch", "git_fetch_prune", "git_fetch_all"} {
		out = append(out, g.run(key, remotes))
	}
	return out
}

func (g gitMenus) remotes() []action {
	return []action{g.run("git_add_remote", true),
		g.choose("git_remove_remote", g.remote(func(name string) func(*ui.Context) {
			return func(*ui.Context) {
				g.v.confirm = confirm{open: true, title: g.t("git_remove_remote"), message: g.t("git_remove_remote_confirm"), target: name, run: func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: "git_remove_remote", Remote: name}) }}
			}
		}), "git_remove_remote_open")}
}

func (g gitMenus) stashes() []action {
	stashes := g.repo().Stashes
	choices := func(run func(string) func(*ui.Context)) []action {
		var out []action
		for index, s := range stashes {
			id := s
			if index < len(g.v.stashIDs) {
				id = g.v.stashIDs[index]
			}
			out = append(out, action{label: s, run: run(id)})
		}
		return out
	}
	has := len(g.entries()) > 0
	out := []action{g.run("git_stash_tracked", has), g.run("git_stash_all", has), g.run("git_stash_staged", g.staged())}
	for _, o := range [][2]string{{"git_stash_apply_latest", "git_stash_apply"}, {"git_stash_pop_latest", "git_stash_pop"}} {
		out = append(out, separatorItem, g.run(o[0], len(stashes) > 0),
			g.choose(o[1], choices(func(id string) func(*ui.Context) {
				return func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: o[1], Target: id}) }
			}), o[1]))
	}
	// The confirmation names the stash by its message, as the list does,
	// and drops it by its ID.
	var drop []action
	for _, choice := range choices(func(id string) func(*ui.Context) {
		return func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: "git_stash_drop", Target: id}) }
	}) {
		drop = append(drop, action{label: choice.label, run: func(*ui.Context) {
			g.v.confirm = confirm{open: true, title: g.t("git_stash_drop"), message: g.t("git_delete_confirm"), target: choice.label, run: choice.run}
		}})
	}
	return append(out, separatorItem, g.choose("git_stash_drop", drop, "git_stash_drop"),
		g.confirm("git_stash_drop_all", "git_delete_confirm", strings.Join(stashes, "\n"), len(stashes) > 0, "git_stash_drop_all_open"),
		separatorItem, g.choose("git_stash_view", choices(func(id string) func(*ui.Context) {
			return func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: "git_stash_view", Target: id}) }
		}), "git_stash_view_open"))
}

func (g gitMenus) tags() []action {
	var del []action
	for _, t := range g.repo().Tags {
		commit := g.v.tagIDs[t]
		del = append(del, action{label: t, run: func(*ui.Context) {
			g.v.confirm = confirm{open: true, title: g.t("git_tag_delete"), message: g.t("git_delete_confirm"), target: t, run: func(c *ui.Context) {
				g.pr.gitAction(c, GitAction{Kind: "git_tag_delete", Target: t, Secondary: commit})
			}}
		}})
	}
	create := g.run("git_tag_create", g.head())
	create.label = g.t("git_tag_create_open")
	return []action{create, g.choose("git_tag_delete", del, "git_tag_delete_open"),
		g.choose("git_tag_delete_remote", g.remote(func(remote string) func(*ui.Context) {
			return func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: "git_tag_delete_remote", Remote: remote}) }
		}), "git_tag_delete_remote_open"),
		separatorItem, g.choose("git_tag_push", g.remote(func(remote string) func(*ui.Context) {
			return func(c *ui.Context) { g.pr.gitAction(c, GitAction{Kind: "git_tag_push", Remote: remote}) }
		}), "git_tag_push_open")}
}

// root is the ⋯ menu.
func (g gitMenus) root() []action {
	repo := g.repo()
	return []action{g.section("git_view_options", g.viewOptions()), separatorItem,
		g.run("git_pull", repo.Upstream != ""), g.push(false), g.run("git_clone", true),
		g.choose("git_checkout", g.checkout(), "git_checkout_open"), g.run("git_fetch", len(repo.Remotes) > 0), separatorItem,
		g.section("git_commit", g.commits()), g.section("git_changes_menu", g.changes()), g.section("git_pull_push", g.network()),
		g.choose("git_manage_branches", g.management(), "git_manage_branches"), g.section("git_remotes", g.remotes()),
		g.section("git_stash_menu", g.stashes()), g.section("git_tags", g.tags()), g.run("worktree_manage", true), separatorItem,
		g.run("git_output", true)}
}

// files is a change's, a folder's or a group's right-click menu, acting
// on every change in it.
func (g gitMenus) files(entries []preview.FileChange, directory bool, directoryPath ...string) []action {
	var paths []string
	for _, e := range entries {
		paths = append(paths, e.Path)
	}
	ignorePaths, ignoreDirectory := paths, false
	if directory && len(directoryPath) > 0 && directoryPath[0] != "" {
		ignorePaths, ignoreDirectory = []string{directoryPath[0]}, true
	}
	var out []action
	if len(entries) == 1 && !directory {
		e := entries[0]
		if e.Unstaged != "D" && e.Staged != "D" {
			out = append(out, g.item("git_open_source", func(*ui.Context) { g.pr.source(e.Path) }, true))
		}
		open := func(scope string) func(*ui.Context) {
			return func(*ui.Context) { g.pr.showChange(e.Path, scope) }
		}
		out = append(out, g.item("git_unstaged", open("unstaged"), e.Unstaged != "" || e.Untracked),
			g.item("git_staged", open("staged"), e.Staged != ""), separatorItem)
	}
	return append(out,
		g.item("git_stage", func(c *ui.Context) {
			g.pr.gitAction(c, GitAction{Kind: "git_stage", Paths: indexPaths(entries, "stage")})
		}, len(indexPaths(entries, "stage")) > 0),
		g.item("git_unstage", func(c *ui.Context) {
			g.pr.gitAction(c, GitAction{Kind: "git_unstage", Paths: indexPaths(entries, "unstage")})
		}, len(indexPaths(entries, "unstage")) > 0),
		g.discard(entries, paths),
		g.item("git_ignore", func(c *ui.Context) {
			g.pr.gitAction(c, GitAction{Kind: "git_ignore", Paths: ignorePaths, Scope: func() string {
				if ignoreDirectory {
					return "directory"
				}
				return "file"
			}()})
		}, len(paths) > 0), separatorItem,
		g.item("git_copy_path", func(c *ui.Context) { c.WriteClipboard(strings.Join(paths, "\n")) }, true))
}

// newFile reports a change HEAD does not hold: an untracked file or a
// staged addition. Restoring HEAD would delete it.
func newFile(e preview.FileChange) bool {
	return e.Staged == "A" || e.Untracked && e.Staged == ""
}

// discard is Discard Changes on a selection. Changes HEAD holds are
// restored from it. A selection with a new file discards as Discard all
// does, the new files moved to Trash, and its confirmation says so.
func (g gitMenus) discard(entries []preview.FileChange, paths []string) action {
	kind, message := "git_discard_changes", "git_discard_confirm"
	if slices.ContainsFunc(entries, newFile) {
		kind, message = "git_discard_all", "git_discard_all_confirm"
	}
	target := strings.Join(paths, "\n")
	return g.item("git_discard_changes", func(*ui.Context) {
		g.v.confirm = confirm{open: true, title: g.t("git_discard_changes"), message: g.t(message), target: target, run: func(c *ui.Context) {
			g.pr.gitAction(c, GitAction{Kind: kind, Target: target, Paths: paths})
		}}
	}, len(paths) > 0)
}

// fill adds actions to a left-click menu.
func fill(c *ui.Context, m *kit.Menu, list []action) {
	for _, a := range list {
		switch {
		case a.sep:
			m.Separator()
		case a.sub != nil:
			m.Submenu(a.label, func(m *kit.Menu) { fill(c, m, a.sub) }).Disabled(a.disabled)
		default:
			it := m.Item(a.label).Checked(a.checked).Disabled(a.disabled)
			if a.icon != nil {
				it.Icon(a.icon)
			}
			if it.Chosen() {
				a.run(c)
			}
		}
	}
}

// fillNative adds actions to a right-click menu, the system's.
func fillNative(c *ui.Context, m *ui.Menu, list []action) {
	for _, a := range list {
		switch {
		case a.sep:
			m.Separator()
		case a.sub != nil:
			m.Submenu(a.label, func(m *ui.Menu) { fillNative(c, m, a.sub) }).Disabled(a.disabled)
		default:
			if m.Item(a.label).Checked(a.checked).Disabled(a.disabled).Chosen() {
				a.run(c)
			}
		}
	}
}
