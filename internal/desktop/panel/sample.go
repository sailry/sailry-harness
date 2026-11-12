package panel

import (
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The demo's Files and Git act on its sample workspace in memory, where the
// application asks its Node: the index, the branches, the stashes, the tags
// and the tree change as the control says and nothing is written. What
// needs the system (another app, a file manager, a download) is not here.

// sampled carries out a control of a sample project: in the demo in memory,
// reporting whether it did; in the preview with its notice.
func sampled(c *ui.Context, run func() bool) {
	if !kit.Demo(c) || !run() {
		notice(c)
	}
}

func covered(path string, paths []string) bool {
	return len(paths) == 0 || slices.ContainsFunc(paths, func(p string) bool {
		return p == "" || path == p || strings.HasPrefix(path, p+"/")
	})
}

// stage and unstage move a change between the work tree and the index.
func stageChange(e *preview.FileChange) {
	if e.Untracked {
		e.Untracked, e.Staged = false, "A"
	} else if e.Unstaged != "" {
		e.Staged = e.Unstaged
	}
	e.Unstaged = ""
}

func unstageChange(e *preview.FileChange) {
	if e.Staged == "A" {
		e.Untracked = true
	} else if e.Staged != "" {
		e.Unstaged = e.Staged
	}
	e.Staged = ""
}

// sampleGit is a Git action on the demo's repository.
func (pr *Project) sampleGit(c *ui.Context, a GitAction) bool {
	w, v := pr.Data, &pr.Git
	repo := &w.Repo
	each := func(fn func(*preview.FileChange)) {
		for i := range w.Changes {
			if covered(w.Changes[i].Path, a.Paths) {
				fn(&w.Changes[i])
			}
		}
	}
	drop := func(match func(preview.FileChange) bool) {
		w.Changes = slices.DeleteFunc(w.Changes, match)
		if len(w.Changes) > 0 && !slices.ContainsFunc(w.Changes, func(e preview.FileChange) bool { return e.Path == v.change }) {
			v.change = w.Changes[0].Path
		}
	}
	current := func() *preview.Branch {
		for i := range repo.Branches {
			if repo.Branches[i].Current {
				return &repo.Branches[i]
			}
		}
		return nil
	}
	// commit puts a commit at the head of the branch checked out.
	commit := func(message string) {
		v.sampleCommits++
		cm := preview.Commit{ID: fmt.Sprintf("%016x", 0xd3a0000000000000+uint64(v.sampleCommits)), Message: message, Time: preview.Now, Refs: []string{"HEAD", "heads/" + repo.Branch}}
		if len(repo.Commits) > 0 {
			head := &repo.Commits[0]
			cm.Author, cm.Parents = head.Author, []string{head.ID}
			head.Refs = slices.DeleteFunc(slices.Clone(head.Refs), func(r string) bool { return slices.Contains(cm.Refs, r) })
		}
		repo.Commits = slices.Insert(repo.Commits, 0, cm)
		if b := current(); b != nil {
			b.Commit = cm.ID[:12]
		}
	}
	// stash puts the matching changes away under a message.
	stash := func(match func(preview.FileChange) bool) {
		var kept []preview.FileChange
		for _, e := range w.Changes {
			if match(e) {
				kept = append(kept, e)
			}
		}
		message := "WIP on " + repo.Branch
		if len(repo.Commits) > 0 {
			message += ": " + repo.Commits[0].ID[:7] + " " + strings.SplitN(repo.Commits[0].Message, "\n", 2)[0]
		}
		repo.Stashes = slices.Insert(repo.Stashes, 0, message)
		v.sampleStashes = slices.Insert(v.sampleStashes, 0, kept)
		drop(match)
	}
	// stashAt is the stash an action names by its message, or the latest.
	stashAt := func() int {
		if a.Target == "" && len(repo.Stashes) > 0 {
			return 0
		}
		return slices.Index(repo.Stashes, a.Target)
	}
	restore := func(i int, pop bool) {
		if i < 0 {
			return
		}
		// The stashes the demo began with hold no changes of its tree.
		for len(v.sampleStashes) < len(repo.Stashes) {
			v.sampleStashes = append(v.sampleStashes, nil)
		}
		for _, e := range v.sampleStashes[i] {
			if !slices.ContainsFunc(w.Changes, func(x preview.FileChange) bool { return x.Path == e.Path }) {
				w.Changes = append(w.Changes, e)
			}
		}
		if pop {
			repo.Stashes = slices.Delete(repo.Stashes, i, i+1)
			v.sampleStashes = slices.Delete(v.sampleStashes, i, i+1)
		}
	}
	synced := func() {
		if b := current(); b != nil {
			b.Tracking = ""
		}
	}
	switch a.Kind {
	case "git_stage", "git_stage_all":
		each(stageChange)
	case "git_unstage", "git_unstage_all":
		each(unstageChange)
	case "git_discard_changes", "git_discard_all":
		drop(func(e preview.FileChange) bool { return covered(e.Path, a.Paths) })
	case "git_commit":
		if a.Amend && len(repo.Commits) > 0 {
			repo.Commits[0].Message = a.Message
		} else {
			commit(a.Message)
		}
		staged := slices.ContainsFunc(w.Changes, func(e preview.FileChange) bool { return e.Staged != "" })
		if a.Scope == "all" || !staged {
			drop(func(preview.FileChange) bool { return true })
		} else {
			drop(func(e preview.FileChange) bool { return e.Staged != "" && e.Unstaged == "" })
			for i := range w.Changes {
				w.Changes[i].Staged = ""
			}
		}
		pr.CommitSaved(a.Message)
		if a.After != "" && a.After != "none" {
			synced()
		}
	case "git_undo_commit":
		if len(repo.Commits) < 2 {
			return false
		}
		refs := repo.Commits[0].Refs
		repo.Commits = repo.Commits[1:]
		repo.Commits[0].Refs = append(slices.Clone(repo.Commits[0].Refs), refs...)
		if b := current(); b != nil {
			b.Commit = repo.Commits[0].ID[:12]
		}
	case "git_cherry_pick":
		at := slices.IndexFunc(repo.Commits, func(cm preview.Commit) bool { return cm.ID == a.Target })
		if at < 0 {
			return false
		}
		commit(repo.Commits[at].Message)
	case "branch_switch", "git_checkout":
		at := slices.IndexFunc(repo.Branches, func(b preview.Branch) bool { return b.Name == a.Target && !b.Remote })
		if at < 0 {
			return false
		}
		for i := range repo.Branches {
			repo.Branches[i].Current = i == at
		}
		repo.Branch, repo.Upstream = a.Target, ""
		if slices.ContainsFunc(repo.Branches, func(b preview.Branch) bool { return b.Remote && b.Name == "origin/"+a.Target }) {
			repo.Upstream = "origin/" + a.Target
		}
		for i := range repo.Commits {
			cm := &repo.Commits[i]
			cm.Refs = slices.DeleteFunc(slices.Clone(cm.Refs), func(r string) bool { return r == "HEAD" })
			if slices.Contains(cm.Refs, "heads/"+a.Target) {
				cm.Refs = append([]string{"HEAD"}, cm.Refs...)
			}
		}
	case "branch_delete", "git_delete_branch", "git_delete_remote_branch":
		repo.Branches = slices.DeleteFunc(repo.Branches, func(b preview.Branch) bool { return b.Name == a.Target && !b.Current })
		for i := range repo.Commits {
			cm := &repo.Commits[i]
			cm.Refs = slices.DeleteFunc(slices.Clone(cm.Refs), func(r string) bool { return r == "heads/"+a.Target || r == "remotes/"+a.Target })
		}
	case "branch_merge":
		commit("Merge branch '" + a.Target + "' into " + repo.Branch)
	case "branch_create", "branch_rename":
		title := "git_new_branch"
		if a.Kind == "branch_rename" {
			title = "git_rename_branch"
		}
		pr.Prompt(title, []string{"settings_name"}, []string{a.Target}, func(values []string, done func(bool)) {
			name := strings.TrimSpace(values[0])
			taken := slices.ContainsFunc(repo.Branches, func(b preview.Branch) bool { return b.Name == name })
			if name == "" || taken {
				done(false)
				return
			}
			if at := slices.IndexFunc(repo.Branches, func(b preview.Branch) bool { return b.Name == a.Target }); a.Kind == "branch_rename" && at >= 0 {
				repo.Branches[at].Name = name
				if repo.Branches[at].Current {
					repo.Branch = name
				}
			} else {
				at := a.Secondary
				if at == "" && len(repo.Commits) > 0 {
					at = repo.Commits[0].ID
				}
				repo.Branches = append(repo.Branches, preview.Branch{Name: name, Commit: at[:min(12, len(at))]})
			}
			for i := range repo.Commits {
				cm := &repo.Commits[i]
				if a.Kind == "branch_rename" {
					if j := slices.Index(cm.Refs, "heads/"+a.Target); j >= 0 {
						cm.Refs = slices.Clone(cm.Refs)
						cm.Refs[j] = "heads/" + name
					}
				} else if a.Secondary == "" && i == 0 || a.Secondary != "" && strings.HasPrefix(cm.ID, a.Secondary) {
					cm.Refs = append(slices.Clone(cm.Refs), "heads/"+name)
				}
			}
			done(true)
		})
	case "git_tag_create":
		pr.Prompt("git_tag_create", []string{"settings_name"}, []string{""}, func(values []string, done func(bool)) {
			name := strings.TrimSpace(values[0])
			if name == "" || slices.Contains(repo.Tags, name) {
				done(false)
				return
			}
			repo.Tags = append(repo.Tags, name)
			for i := range repo.Commits {
				if cm := &repo.Commits[i]; a.Secondary == "" && i == 0 || a.Secondary != "" && cm.ID == a.Secondary {
					cm.Refs = append(slices.Clone(cm.Refs), "tags/"+name)
				}
			}
			done(true)
		})
	case "git_tag_delete":
		repo.Tags = slices.DeleteFunc(repo.Tags, func(t string) bool { return t == a.Target })
		for i := range repo.Commits {
			cm := &repo.Commits[i]
			cm.Refs = slices.DeleteFunc(slices.Clone(cm.Refs), func(r string) bool { return r == "tags/"+a.Target })
		}
	case "git_remove_remote":
		repo.Remotes = slices.DeleteFunc(repo.Remotes, func(r preview.Remote) bool { return r.Name == a.Target || r.Name == a.Remote })
	case "git_stash_all":
		stash(func(preview.FileChange) bool { return true })
	case "git_stash_tracked":
		stash(func(e preview.FileChange) bool { return !e.Untracked })
	case "git_stash_staged":
		stash(func(e preview.FileChange) bool { return e.Staged != "" })
	case "git_stash_apply", "git_stash_apply_latest":
		restore(stashAt(), false)
	case "git_stash_pop", "git_stash_pop_latest":
		restore(stashAt(), true)
	case "git_stash_drop":
		if i := stashAt(); i >= 0 {
			repo.Stashes = slices.Delete(repo.Stashes, i, i+1)
			if i < len(v.sampleStashes) {
				v.sampleStashes = slices.Delete(v.sampleStashes, i, i+1)
			}
		}
	case "git_stash_drop_all":
		repo.Stashes, v.sampleStashes = nil, nil
	case "git_fetch", "git_fetch_all", "git_fetch_prune", "git_fetch_all_prune", "git_tag_push", "git_tag_delete_remote":
		// The remotes hold nothing the demo's repository lacks.
	case "git_pull", "git_pull_rebase", "git_pull_from", "git_push", "git_push_to", "git_force_push", "git_sync", "git_sync_rebase", "git_pull_push":
		synced()
	default:
		return false
	}
	return true
}

// sampleFile is a Files action on the demo's tree.
func (pr *Project) sampleFile(c *ui.Context, a FileAction) bool {
	root := pr.Data.Files
	if root == nil {
		return false
	}
	parent := func(path string) *preview.FileNode {
		if f := findFile(root, parentDir(path)); f != nil && f.Children != nil {
			return f
		}
		return nil
	}
	remove := func(path string) *preview.FileNode {
		dir := parent(path)
		if dir == nil {
			return nil
		}
		i := slices.IndexFunc(dir.Children, func(f *preview.FileNode) bool { return f.Path == path })
		if i < 0 {
			return nil
		}
		f := dir.Children[i]
		dir.Children = slices.Delete(dir.Children, i, i+1)
		return f
	}
	// place puts a node in a folder under a name no sibling has.
	place := func(dir *preview.FileNode, f *preview.FileNode, name string) {
		base, n := name, 1
		for slices.ContainsFunc(dir.Children, func(x *preview.FileNode) bool { return x.Name == name }) {
			n++
			name = fmt.Sprintf("%s %d", base, n)
		}
		f.Name = name
		repath(f, dir.Path+"/"+name)
		dir.Children = append(dir.Children, f)
		slices.SortStableFunc(dir.Children, func(x, y *preview.FileNode) int {
			if (x.Children != nil) != (y.Children != nil) {
				if x.Children != nil {
					return -1
				}
				return 1
			}
			return strings.Compare(x.Name, y.Name)
		})
	}
	folder := func() *preview.FileNode {
		if f := findFile(root, a.Directory); f != nil && f.Children != nil {
			return f
		}
		return root
	}
	switch a.Kind {
	case "files_new_file", "files_new_directory":
		label := "files_new_file"
		if a.Kind == "files_new_directory" {
			label = "files_new_directory"
		}
		dir := folder()
		pr.Prompt(label, []string{"settings_name"}, []string{""}, func(values []string, done func(bool)) {
			name := strings.TrimSpace(values[0])
			if name == "" || strings.Contains(name, "/") {
				done(false)
				return
			}
			f := &preview.FileNode{}
			if a.Kind == "files_new_directory" {
				f.Children = []*preview.FileNode{}
			}
			place(dir, f, name)
			done(true)
		})
	case "files_rename":
		if len(a.Paths) != 1 {
			return false
		}
		f := findFile(root, a.Paths[0])
		dir := parent(a.Paths[0])
		if f == nil || dir == nil {
			return false
		}
		pr.Prompt("files_rename", []string{"settings_name"}, []string{f.Name}, func(values []string, done func(bool)) {
			name := strings.TrimSpace(values[0])
			if name == "" || strings.Contains(name, "/") {
				done(false)
				return
			}
			if name != f.Name {
				remove(f.Path)
				place(dir, f, name)
			}
			done(true)
		})
	case "files_trash":
		for _, path := range a.Paths {
			remove(path)
			pr.Files.Close(path)
		}
		pr.PruneFileSelection()
	case "files_move":
		dir := folder()
		for _, path := range a.Paths {
			if dir.Path == path || strings.HasPrefix(dir.Path, path+"/") || parentDir(path) == dir.Path {
				continue
			}
			if f := remove(path); f != nil {
				place(dir, f, f.Name)
			}
		}
		pr.PruneFileSelection()
	case "files_copy", "files_cut":
		pr.Files.sampleClip, pr.Files.sampleCut = slices.Clone(a.Paths), a.Kind == "files_cut"
	case "files_paste":
		dir := folder()
		for _, path := range pr.Files.sampleClip {
			if pr.Files.sampleCut {
				if dir.Path == path || strings.HasPrefix(dir.Path, path+"/") {
					continue
				}
				if f := remove(path); f != nil {
					place(dir, f, f.Name)
				}
			} else if f := findFile(root, path); f != nil {
				place(dir, cloneFile(f), f.Name)
			}
		}
		if pr.Files.sampleCut {
			pr.Files.sampleClip = nil
		}
	default:
		return false
	}
	return true
}

func repath(f *preview.FileNode, path string) {
	f.Path = path
	for _, k := range f.Children {
		repath(k, path+"/"+k.Name)
	}
}

func cloneFile(f *preview.FileNode) *preview.FileNode {
	out := *f
	if f.Children != nil {
		out.Children = make([]*preview.FileNode, 0, len(f.Children))
		for _, k := range f.Children {
			out.Children = append(out.Children, cloneFile(k))
		}
	}
	return &out
}
