package live

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/picker"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// files is the file system of a host for the in-app picker: the local Node's
// or a paired Host's, both read through their controller, so a remote
// directory is browsed on the Host that owns it. It is false while the host
// has no controller.
func (r *Runtime) files(host string) (picker.Source, bool) {
	owner := r.runtimeFor(host)
	if owner == nil {
		return picker.Source{}, false
	}
	return picker.Source{Browse: owner.browseFiles, Manage: owner.manageFiles}, true
}

// browseProblem is the picker's reason for a read that failed: the directory
// is not there, the Node may not read it, or its host did not answer. Any
// other fault is the plain failure.
func browseProblem(err error) string {
	switch faultCode(err) {
	case protocol.ErrorCodeNotFound:
		return "directory_missing"
	case protocol.ErrorCodePermissionDenied:
		return "directory_denied"
	case protocol.ErrorCodeUnavailable, "":
		return "directory_host_unavailable"
	}
	return "directory_failed"
}

// browseFiles reads a directory of this runtime's Node, page after page, and
// answers on the UI thread. A page that continues another directory fails the
// read rather than mixing two listings.
func (r *Runtime) browseFiles(directory string, done func(picker.Listing, string)) {
	fail := func(problem string) { r.enqueue(func(*ui.Context) { done(picker.Listing{}, problem) }) }
	started := r.async(func() {
		var listing picker.Listing
		var target *string
		if directory != "" {
			target = &directory
		}
		var after *protocol.DirectoryCursor
		for {
			output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "browse_files", Data: protocol.CommandBrowseFilesPayload{Directory: target, After: after}})
			page, valid := detail[protocol.FileBrowserListing](output.Data)
			if err != nil {
				fail(browseProblem(err))
				return
			}
			if !valid || after != nil && page.Directory.Path != listing.Path {
				fail("directory_failed")
				return
			}
			if after == nil {
				listing = picker.Listing{Separator: string(page.Separator), Path: page.Directory.Path}
				if page.Parent != nil {
					listing.Parent = *page.Parent
				}
				for _, location := range page.Locations {
					place := picker.Location{Kind: string(location.Kind), Name: location.Name, Path: location.Path}
					if location.Medium != nil {
						place.Medium = string(*location.Medium)
					}
					listing.Locations = append(listing.Locations, place)
				}
				target = &listing.Path
			}
			for _, entry := range page.Directory.Entries {
				item := picker.Entry{Name: entry.Name, Kind: string(entry.Kind), Size: entry.Size}
				if entry.ModifiedMs != nil {
					item.Modified = time.UnixMilli(int64(*entry.ModifiedMs))
				}
				listing.Entries = append(listing.Entries, item)
			}
			if after = page.Directory.Next; after == nil {
				break
			}
		}
		r.enqueue(func(*ui.Context) { done(listing, "") })
	})
	if !started {
		fail("directory_host_unavailable")
	}
}

// manageFiles applies a picker action on this runtime's Node. An unknown or
// unavailable outcome outranks a conflict, which outranks any other failure.
func (r *Runtime) manageFiles(action picker.Action, done func(string)) {
	var data protocol.FileBrowserAction
	switch action.Kind {
	case "create":
		data = protocol.FileBrowserAction{Kind: "create_directory", Data: protocol.FileBrowserActionCreateDirectoryPayload{Path: action.Path}}
	case "rename":
		data = protocol.FileBrowserAction{Kind: "rename", Data: protocol.FileBrowserActionRenamePayload{From: action.Path, To: action.To}}
	case "transfer":
		data = protocol.FileBrowserAction{Kind: "transfer", Data: protocol.FileBrowserActionTransferPayload{Paths: action.Paths, Destination: action.Destination, Cut: action.Cut, Conflict: protocol.FileBrowserConflict(action.Conflict)}}
	case "trash":
		data = protocol.FileBrowserAction{Kind: "trash", Data: protocol.FileBrowserActionTrashPayload{Paths: action.Paths}}
	}
	finish := func(problem string) { r.enqueue(func(*ui.Context) { done(problem) }) }
	started := r.async(func() {
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "manage_files", Data: data})
		codes := []protocol.ErrorCode{}
		if err != nil {
			codes = append(codes, faultCode(err))
		} else if outcomes, valid := detail[[]protocol.FileBrowserOutcome](output.Data); valid {
			for _, outcome := range outcomes {
				if outcome.Error != nil {
					codes = append(codes, outcome.Error.Code)
				}
			}
		} else {
			codes = append(codes, protocol.ErrorCodeOutcomeUnknown)
		}
		problem := ""
		for _, code := range codes {
			switch {
			case code == protocol.ErrorCodeOutcomeUnknown || code == protocol.ErrorCodeUnavailable || code == "":
				problem = "directory_outcome_unknown"
			case code == protocol.ErrorCodeConflict && problem != "directory_outcome_unknown":
				problem = "files_paste_conflict"
			case problem == "":
				problem = "directory_action_failed"
			}
		}
		finish(problem)
	})
	if !started {
		finish("directory_outcome_unknown")
	}
}

// bindPicker gives the picker the saved view preferences and saves them as
// the user changes them.
func (r *Runtime) bindPicker() {
	state := r.App.Sidebar.Picker
	if saved := r.Preferences.Data.FilePicker; saved != nil {
		state.Grid, state.Hidden = saved.Grid, saved.Hidden
	}
	// The frame's end saves what changed.
	state.Changed = func() {
		r.Preferences.Data.FilePicker = &preferences.FilePicker{Grid: state.Grid, Hidden: state.Hidden}
	}
}

// chooseFile opens the picker on this machine's files for a settings form
// that reads the chosen file here, as a plugin package is read before its
// upload. name is the local host's as the form shows it; valid reports
// whether the form still wants the answer.
func (r *Runtime) chooseFile(name, title, initial string, valid func() bool, selected func(string)) {
	source, ok := r.files("")
	if !ok {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	}
	r.App.Sidebar.Picker.Open(picker.Options{Mode: picker.File, Title: title, Confirm: "directory_open", Initial: initial,
		Source: source, Host: name, Valid: valid, Choose: func(paths []string) bool {
			if !valid() || len(paths) != 1 {
				return false
			}
			selected(paths[0])
			return true
		}})
}

// chooseDirectory opens the picker on a host's directories for a settings
// form, under the host's name as the form shows it, which the picker shows
// for a host other than this device; valid reports whether the form still
// wants the answer.
func (r *Runtime) chooseDirectory(host, name, title, initial string, valid func() bool, selected func(string)) {
	source, ok := r.files(host)
	if !ok {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	}
	r.App.Sidebar.Picker.Open(picker.Options{Mode: picker.Directory, Title: title, Confirm: "directory_title", Initial: initial,
		Source: source, Host: name, Remote: r.runtimeFor(host) != r.localRuntime(), Valid: valid, Choose: func(paths []string) bool {
			if !valid() || len(paths) != 1 {
				return false
			}
			selected(paths[0])
			return true
		}})
}
