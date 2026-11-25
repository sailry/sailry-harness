package live

import (
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/link"
	nodefiles "github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
	filesPage "github.com/sailry/sailry-harness/modules/files/desktop"
)

func workspaceFilesWindow(root *Runtime, project *workspaceProject, before func(*ui.Context)) *ui.Tester {
	window := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queue {
			apply(c)
		}
		if before != nil {
			before(c)
		}
		project.resourceFrame()
		root.App.Panel.Sync(project.panel)
		ui.Row(c).FillWidth().FillHeight().Children(func() {
			ui.Column(c.Key("document")).Grow(1).MinWidth(0).Children(func() {
				filesPage.Render(c, project.panel)
			})
			ui.Column(c.Key("details")).Width(300).Children(func() {
				filesPage.Details(c, project.panel)
			})
		})
		kit.ToastHost(c)
	}, 1000, 700)
	window.SetLocale("en")
	return window
}

func workspaceFilesAdmission(request protocol.Request, out protocol.Output, err error) (link.Admission, error) {
	response := link.Response{Output: out}
	if err != nil {
		var fault *protocol.Fault
		if !errors.As(err, &fault) {
			return link.Admission{}, err
		}
		response.Fault = fault
	}
	completion := make(chan link.Response, 1)
	completion <- response
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

func TestWorkspaceMissingOpenFileWatch(t *testing.T) {
	for _, dirty := range []bool{false, true} {
		t.Run(map[bool]string{false: "clean", true: "draft"}[dirty], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t, shell.Assemble(filesPage.Register))
				root := fixture.root
				root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
				project := root.workspaceFor(fixture.main)
				project.initialized, project.release = true, func() {}
				root.workspace.selected = fixture.main.ID
				directory, err := filepath.EvalSymlinks(t.TempDir())
				if err != nil {
					t.Fatal(err)
				}
				const relative = "plans/host-owned-accounts.md"
				if err := os.Mkdir(filepath.Join(directory, "plans"), 0o700); err != nil {
					t.Fatal(err)
				}
				for name, text := range map[string]string{".gitignore": "plans/\n", relative: "original\n"} {
					if err := os.WriteFile(filepath.Join(directory, name), []byte(text), 0o600); err != nil {
						t.Fatal(err)
					}
				}
				var mu sync.Mutex
				reads := 0
				fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
					if request.Command.Kind == "read_file" {
						mu.Lock()
						reads++
						mu.Unlock()
					}
					command := request.Command
					switch command.Kind {
					case "read_file":
						input, _ := detail[protocol.CommandReadFilePayload](command.Data)
						command.Data = &input
					case "list_directory":
						input, _ := detail[protocol.CommandListDirectoryPayload](command.Data)
						command.Data = &input
					}
					out, err := nodefiles.Inspect(root.ctx, directory, command, core.ManualFiles, nil)
					return workspaceFilesAdmission(request, out, err)
				}
				window := workspaceFilesWindow(root, project, nil)
				settle := func() {
					for range 4 {
						synctest.Wait()
						window.Frame()
					}
				}
				project.readDirectory("", false)
				settle()
				project.panel.FileOpen(relative)
				settle()
				original := project.files[relative]
				if original.Revision == nil || original.Text != "original\n" {
					t.Fatalf("manual file entry did not load the ignored file: content=%#v failed_reads=%v commands=%v", original, project.failedReads, fixture.source.commandKinds())
				}
				if dirty {
					project.panel.FileChanged(relative, "my draft")
				}
				if err := os.Remove(filepath.Join(directory, relative)); err != nil {
					t.Fatal(err)
				}
				for range 8 {
					project.pendingFiles = true
					settle()
				}
				mu.Lock()
				gotReads := reads
				mu.Unlock()
				if gotReads != 2 || !project.entryIsMissing(relative) {
					t.Fatalf("watch reread a missing open path: reads=%d missing=%t", gotReads, project.entryIsMissing(relative))
				}
				if dirty {
					text, changed := project.panel.FileDraft(relative)
					if !changed || text != "my draft\n" || project.files[relative].Revision != original.Revision || !project.changedFiles[relative].missing || workspaceFileNode(project.data.Files, relative) == nil {
						t.Fatal("a missing file lost its visible draft or the draft's source")
					}
				} else if count := countText(window.Texts(), i18n.In("en", "workspace_action_failed")); count != 1 {
					t.Fatalf("the first real missing read was not reported once: %q", window.Texts())
				}
				if err := os.WriteFile(filepath.Join(directory, relative), []byte("recreated\n"), 0o600); err != nil {
					t.Fatal(err)
				}
				project.pendingFiles = true
				settle()
				settle()
				if project.entryIsMissing(relative) {
					t.Fatal("a listing did not release the recreated path")
				}
				if dirty {
					if text, changed := project.panel.FileDraft(relative); !changed || text != "my draft\n" || project.changedFiles[relative].content.Text != "recreated\n" {
						t.Fatal("recreation replaced the draft or kept the missing source")
					}
				} else if project.files[relative].Text != "recreated\n" {
					t.Fatal("the recreated file did not reload")
				}
			})
		})
	}
}

func TestWorkspaceMissingDirectoryRetiresOpenFiles(t *testing.T) {
	for _, dirty := range []bool{false, true} {
		t.Run(map[bool]string{false: "clean", true: "draft"}[dirty], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t, shell.Assemble(filesPage.Register))
				root := fixture.root
				root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
				project := root.workspaceFor(fixture.main)
				project.initialized, project.release = true, func() {}
				root.workspace.selected = fixture.main.ID
				const relative = "plans/host-owned-accounts.md"
				project.fileLoaded(relative, protocol.FileContent{Path: relative, Text: "original\n", Revision: new("r1")})
				project.panel.Files.Show(relative)
				if dirty {
					project.panel.FileChanged(relative, "my draft")
				}
				oldRead := make(chan link.Response, 1)
				var mu sync.Mutex
				reads, planReads, restored := 0, 0, false
				fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
					mu.Lock()
					defer mu.Unlock()
					switch request.Command.Kind {
					case "read_file":
						reads++
						if reads == 1 {
							return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: oldRead}, nil
						}
						if !restored {
							return workspaceFilesAdmission(request, protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "path not found"})
						}
						return workspaceFilesAdmission(request, protocol.Output{Kind: "file_content", Data: protocol.FileContent{Path: relative, Text: "restored\n", Revision: new("r2")}}, nil)
					case "list_directory":
						input, _ := detail[protocol.CommandListDirectoryPayload](request.Command.Data)
						entries := []protocol.FileEntry{}
						if input.Path == "plans" {
							planReads++
							if !restored {
								return workspaceFilesAdmission(request, protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "directory not found"})
							}
							entries = append(entries, protocol.FileEntry{Name: "host-owned-accounts.md", Kind: protocol.EntryKindFile})
						} else if restored {
							entries = append(entries, protocol.FileEntry{Name: "plans", Kind: protocol.EntryKindDirectory})
						}
						return workspaceFilesAdmission(request, protocol.Output{Kind: "directory", Data: protocol.Directory{Path: input.Path, Entries: entries}}, nil)
					}
					return link.Admission{}, errors.New("unexpected file command")
				}
				window := workspaceFilesWindow(root, project, nil)
				settle := func() {
					for range 4 {
						synctest.Wait()
						window.Frame()
					}
				}
				if dirty {
					project.checkDraft("draft", relative, nil)
				} else {
					project.loadFile(relative)
				}
				project.readDirectory("plans", true)
				settle()
				oldRead <- link.Response{Output: protocol.Output{Kind: "file_content", Data: protocol.FileContent{Path: relative, Text: "obsolete\n", Revision: new("old")}}}
				settle()
				for range 4 {
					project.pendingFiles = true
					project.actions().ReadDirectory("plans")
					settle()
				}
				mu.Lock()
				gotReads, gotPlanReads := reads, planReads
				mu.Unlock()
				if gotReads != 1 || gotPlanReads != 1 || !project.entryIsMissing(relative) || window.HasText(i18n.In("en", "workspace_action_failed")) {
					t.Fatalf("a missing directory retried or accepted its obsolete child read: files=%d directories=%d", gotReads, gotPlanReads)
				}
				if dirty {
					if text, changed := project.panel.FileDraft(relative); !changed || text != "my draft\n" || !project.changedFiles[relative].missing || workspaceString(project.files[relative].Revision) != "r1" || workspaceFileNode(project.data.Files, relative) == nil {
						t.Fatal("a missing directory lost its draft or failed to mark the missing source")
					}
				} else if _, loaded := project.files[relative]; loaded {
					t.Fatal("a missing directory kept its child's obsolete content")
				}
				mu.Lock()
				restored = true
				mu.Unlock()
				project.pendingFiles = true
				settle()
				settle()
				if project.entryIsMissing(relative) {
					t.Fatal("restoring the directory did not release its open child")
				}
				if dirty {
					if text, changed := project.panel.FileDraft(relative); !changed || text != "my draft\n" || project.changedFiles[relative].content.Text != "restored\n" {
						t.Fatal("restoring the directory replaced its draft or failed to update the disk source")
					}
				} else if project.files[relative].Text != "restored\n" {
					t.Fatal("the open child did not reload after its directory returned")
				}
			})
		})
	}
}

func TestWorkspaceMissingDraftSaveRestoresFile(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		fixture := newPluginLocationTestFixture(t, shell.Assemble(filesPage.Register))
		root := fixture.root
		root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
		project := root.workspaceFor(fixture.main)
		project.initialized, project.release = true, func() {}
		const relative = "plans/host-owned-accounts.md"
		project.fileLoaded(relative, protocol.FileContent{Path: relative, Text: "original\n", Revision: new("r1")})
		project.panel.Files.Show(relative)
		project.panel.FileChanged(relative, "my draft")
		project.entryMissing("plans")
		var mu sync.Mutex
		var writes []protocol.CommandWriteFilePayload
		fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
			mu.Lock()
			defer mu.Unlock()
			switch request.Command.Kind {
			case "write_file":
				input, _ := detail[protocol.CommandWriteFilePayload](request.Command.Data)
				writes = append(writes, input)
				if input.ExpectedRevision != nil {
					return workspaceFilesAdmission(request, protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "path not found"})
				}
				return workspaceFilesAdmission(request, protocol.Output{Kind: "file_written", Data: protocol.FileWritten{Path: relative, Revision: "r2", Size: uint64(len(input.Text))}}, nil)
			case "read_file":
				return workspaceFilesAdmission(request, protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "path not found"})
			}
			return link.Admission{}, errors.New("unexpected file command")
		}
		var requests []kit.Confirmation
		var answer func(int)
		action := true
		window := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			kit.BindConfirmations(c, func(request kit.Confirmation, done func(int)) { requests = append(requests, request); answer = done })
			root.mu.Lock()
			queue := root.queue
			root.queue = nil
			root.mu.Unlock()
			for _, apply := range queue {
				apply(c)
			}
			if action {
				action = false
				project.saveFile(c, relative, "my draft\n")
			}
			filesPage.Render(c, project.panel)
			kit.ToastHost(c)
		}, 1000, 700)
		window.SetLocale("en")
		settle := func() {
			for range 4 {
				synctest.Wait()
				window.Frame()
			}
		}
		settle()
		if len(requests) != 1 || len(requests[0].Buttons) != 3 || requests[0].Buttons[2] != i18n.In("en", "files_overwrite") {
			t.Fatal("saving a missing file did not retain the explicit overwrite choice")
		}
		answer(2)
		settle()
		mu.Lock()
		gotWrites := append([]protocol.CommandWriteFilePayload(nil), writes...)
		mu.Unlock()
		if len(gotWrites) != 2 || workspaceString(gotWrites[0].ExpectedRevision) != "r1" || gotWrites[1].ExpectedRevision != nil || gotWrites[1].Text != "my draft\n" {
			t.Fatalf("restoring the file did not preserve the revision check and explicit overwrite: %#v", gotWrites)
		}
		if project.entryIsMissing(relative) || project.entryIsMissing("plans") || len(project.changedFiles) != 0 || workspaceString(project.files[relative].Revision) != "r2" {
			t.Fatal("the confirmed restored file or its parent stayed marked as missing")
		}
		if text, dirty := project.panel.FileDraft(relative); dirty || text != "my draft\n" || !window.HasText(i18n.In("en", "workspace_saved")) {
			t.Fatal("restoring the file did not settle the saved draft")
		}
	})
}

func TestWorkspaceTrashRetiresReads(t *testing.T) {
	for _, dirty := range []bool{false, true} {
		t.Run(map[bool]string{false: "clean", true: "draft"}[dirty], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t, shell.Assemble(filesPage.Register))
				root := fixture.root
				root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
				project := root.workspaceFor(fixture.main)
				project.initialized, project.release = true, func() {}
				root.workspace.selected = fixture.main.ID
				const relative = "plans/host-owned-accounts.md"
				project.fileLoaded(relative, protocol.FileContent{Path: relative, Text: "original\n", Revision: new("r1")})
				project.panel.Files.Show(relative)
				if dirty {
					project.panel.FileChanged(relative, "my draft")
				}
				oldRead, oldDirectory, trash := make(chan link.Response, 1), make(chan link.Response, 1), make(chan link.Response, 1)
				var mu sync.Mutex
				readStarted, directoryStarted, deleted := false, false, false
				planReads := 0
				fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
					mu.Lock()
					defer mu.Unlock()
					completion := trash
					switch request.Command.Kind {
					case "read_file":
						if readStarted {
							return workspaceFilesAdmission(request, protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "path not found"})
						}
						readStarted, completion = true, oldRead
					case "list_directory":
						input, _ := detail[protocol.CommandListDirectoryPayload](request.Command.Data)
						if input.Path == "plans" {
							planReads++
						}
						if input.Path == "plans" && !directoryStarted {
							directoryStarted, completion = true, oldDirectory
							break
						}
						entries := []protocol.FileEntry{}
						if !deleted && input.Path == "" {
							entries = append(entries, protocol.FileEntry{Name: "plans", Kind: protocol.EntryKindDirectory})
						}
						return workspaceFilesAdmission(request, protocol.Output{Kind: "directory", Data: protocol.Directory{Path: input.Path, Entries: entries}}, nil)
					case "trash_entry":
					case "inspect_git":
						return workspaceFilesAdmission(request, protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}, nil)
					default:
						return link.Admission{}, errors.New("unexpected file command")
					}
					return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
				}
				var requests []kit.Confirmation
				var answer func(int)
				action := true
				window := workspaceFilesWindow(root, project, func(c *ui.Context) {
					kit.BindConfirmations(c, func(request kit.Confirmation, done func(int)) { requests = append(requests, request); answer = done })
					if action {
						action = false
						project.actions().File(c, panel.FileAction{Kind: "files_trash", Paths: []string{"plans"}})
					}
				})
				settle := func() { synctest.Wait(); window.Frame(); synctest.Wait() }
				if dirty {
					project.checkDraft("draft", relative, nil)
				} else {
					project.loadFile(relative)
				}
				project.readDirectory("plans", true)
				settle()
				if len(requests) != 1 || !strings.Contains(requests[0].Message, "plans") || slices.Contains(fixture.source.commandKinds(), "trash_entry") {
					t.Fatal("trash bypassed its native destructive confirmation")
				}
				answer(1)
				settle()
				if !project.trashing["plans"] {
					t.Fatal("confirmed trash did not enter its file operation")
				}
				// The read can observe absence before the trash receipt arrives.
				oldRead <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "path not found"}}
				settle()
				mu.Lock()
				deleted = true
				mu.Unlock()
				trash <- link.Response{Output: protocol.Output{Kind: "entry_trashed", Data: protocol.OutputEntryTrashedPayload{Path: "plans"}}}
				settle()
				// An older successful listing must not recreate the deleted tree.
				oldDirectory <- link.Response{Output: protocol.Output{Kind: "directory", Data: protocol.Directory{Path: "plans", Entries: []protocol.FileEntry{{Name: "host-owned-accounts.md", Kind: protocol.EntryKindFile}}}}}
				settle()
				if window.HasText(i18n.In("en", "workspace_action_failed")) || !window.HasText(i18n.In("en", "workspace_completed")) {
					t.Fatalf("trash was misreported by its obsolete read: %q", window.Texts())
				}
				if dirty {
					if text, changed := project.panel.FileDraft(relative); !changed || text != "my draft\n" || !project.changedFiles[relative].missing || project.files[relative].Revision == nil || workspaceFileNode(project.data.Files, relative) == nil {
						t.Fatal("confirmed trash discarded its unsaved draft")
					}
				} else if _, loaded := project.files[relative]; loaded || slices.Contains(project.panel.Files.Open(), relative) || slices.Contains(root.App.Panel.Tabs, panel.Tab{Kind: panel.Files, ID: relative}) || workspaceFileNode(project.data.Files, relative) != nil {
					t.Fatal("confirmed trash kept a clean editor or stale tree entry")
				}
				for range 4 {
					project.pendingFiles = true
					project.actions().ReadDirectory("plans")
					settle()
				}
				mu.Lock()
				gotPlanReads := planReads
				mu.Unlock()
				if gotPlanReads != 1 {
					t.Fatal("an expanded tree reread its confirmed deleted directory")
				}
				if countText(fixture.source.commandKinds(), "read_file") != 1 {
					t.Fatal("a later watch reread the confirmed deleted file")
				}
				if dirty {
					project.panel.DropFileDraft(relative)
					project.readFile(relative)
					if _, loaded := project.files[relative]; loaded || len(project.changedFiles) != 0 {
						t.Fatal("discarding the retained draft restored the deleted source")
					}
				}
			})
		})
	}
}

func TestWorkspaceTrashFailureAndRecovery(t *testing.T) {
	for _, unknown := range []bool{false, true} {
		t.Run(map[bool]string{false: "failed", true: "uncertain"}[unknown], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				fixture := newPluginLocationTestFixture(t, shell.Assemble(filesPage.Register))
				root := fixture.root
				root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
				project := root.workspaceFor(fixture.main)
				project.initialized, project.release = true, func() {}
				const relative = "plans/host-owned-accounts.md"
				project.fileLoaded(relative, protocol.FileContent{Path: relative, Text: "original\n", Revision: new("r1")})
				project.panel.Files.Show(relative)
				fixture.source.outcome = protocol.RequestOutcome{Kind: "admitted"}
				deleted := false
				fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
					if request.Command.Kind == "read_file" {
						return workspaceFilesAdmission(request, protocol.Output{Kind: "file_content", Data: protocol.FileContent{Path: relative, Text: "original\n", Revision: new("r1")}}, nil)
					}
					if request.Command.Kind == "list_directory" {
						input, _ := detail[protocol.CommandListDirectoryPayload](request.Command.Data)
						entries := []protocol.FileEntry{{Name: "plans", Kind: protocol.EntryKindDirectory}}
						if input.Path == "plans" {
							entries = nil
							if !deleted {
								entries = []protocol.FileEntry{{Name: "host-owned-accounts.md", Kind: protocol.EntryKindFile}}
							}
						}
						return workspaceFilesAdmission(request, protocol.Output{Kind: "directory", Data: protocol.Directory{Path: input.Path, Entries: entries}}, nil)
					}
					if request.Command.Kind == "inspect_git" {
						return workspaceFilesAdmission(request, protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}, nil)
					}
					if request.Command.Kind != "trash_entry" {
						return link.Admission{}, errors.New("unexpected file command")
					}
					code := protocol.ErrorCodePermissionDenied
					if unknown {
						code = protocol.ErrorCodeOutcomeUnknown
						fixture.source.prepared[request.ID] = request
					}
					return workspaceFilesAdmission(request, protocol.Output{}, &protocol.Fault{Code: code, Message: "fixture failure"})
				}
				var requests []kit.Confirmation
				var answer func(int)
				action := true
				window := workspaceFilesWindow(root, project, func(c *ui.Context) {
					kit.BindConfirmations(c, func(request kit.Confirmation, done func(int)) { requests = append(requests, request); answer = done })
					if action {
						action = false
						project.actions().File(c, panel.FileAction{Kind: "files_trash", Paths: []string{relative}})
					}
				})
				settle := func() {
					for range 4 {
						synctest.Wait()
						window.Frame()
					}
				}
				settle()
				answer(1)
				settle()
				name := "workspace_action_failed"
				if unknown {
					name = "workspace_action_unknown"
				}
				if !window.HasText(i18n.In("en", name)) || window.HasText(i18n.In("en", "workspace_completed")) || project.entryIsMissing(relative) || !slices.Contains(project.panel.Files.Open(), relative) {
					t.Fatalf("a non-successful trash retired its editor or lost its true outcome: %q", window.Texts())
				}
				if !unknown {
					answer(0)
					settle()
					return
				}
				if len(requests) != 2 || len(requests[1].Buttons) != 2 || requests[1].Buttons[1] != i18n.In("en", "files_check_result") {
					t.Fatal("uncertain trash did not offer inspection of the original request")
				}
				for index, failedLookup := range []bool{false, true} {
					if failedLookup {
						fixture.source.pending = make(chan link.Response, 1)
						fixture.source.pending <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture inspection unavailable"}}
					}
					answer(1)
					settle()
					fixture.source.pending = nil
					kinds := fixture.source.commandKinds()
					if countText(kinds, "trash_entry") != 1 || len(root.uncertain) != 1 || len(project.uncertainOperations) != 1 || len(requests) != index+3 || len(requests[len(requests)-1].Buttons) != 2 || requests[len(requests)-1].Buttons[1] != i18n.In("en", "files_check_result") || project.entryIsMissing(relative) || !slices.Contains(project.panel.Files.Open(), relative) {
						t.Fatalf("pending or unavailable inspection replayed trash or lost its original recovery: failed_lookup=%t commands=%v", failedLookup, kinds)
					}
				}
				fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "entry_trashed", Data: protocol.OutputEntryTrashedPayload{Path: relative}})
				deleted = true
				answer(1)
				settle()
				kinds := fixture.source.commandKinds()
				if countText(kinds, "trash_entry") != 1 || countText(kinds, "inspect_request") != 4 || len(root.uncertain) != 0 || len(project.uncertainOperations) != 0 || !project.entryIsMissing(relative) || slices.Contains(project.panel.Files.Open(), relative) || !window.HasText(i18n.In("en", "workspace_completed")) {
					t.Fatalf("result inspection replayed trash or failed to retire its confirmed editor: %v", kinds)
				}
			})
		})
	}
}
