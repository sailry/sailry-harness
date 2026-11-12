package live

import (
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// attachmentPickerCommands drives Add attachment in the live window: it opens
// the window's in-app picker in its multi-file mode, not the system's dialog,
// lists this machine's files through the local Node at the conversation's
// worktree, uploads what is chosen and attaches nothing on Cancel. It is a
// step of TestComposerSendRouting.
func attachmentPickerCommands(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"notes.md", "plan.md"} {
		if err := os.WriteFile(filepath.Join(root, name), []byte("# "+name+"\n"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	node, project, worktree, provider, session := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}, protocol.ProviderID{4}, protocol.SessionID{5}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider, "fixture", "code", "ask"
	config.Effort = protocol.Effort{Kind: "default"}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Workspace", Path: root}}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: root, Main: true}}
	snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Revision: 7, Enabled: true, Name: "Provider", Api: "responses", Authentication: "api_key", Models: []protocol.ConversationModel{{ID: "fixture", DefaultEffort: protocol.Effort{Kind: "default"}}}}}
	snapshot.Sessions = []protocol.Session{{ID: session, Revision: 6, Worktree: worktree, Project: &project, Config: config, Roles: protocol.DefaultRoleSnapshot()}}
	snapshot.Sessions[0].Activity.Title = "Attachment session"
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.conversation = protocol.DefaultConversationSnapshot()
	transport.conversation.Node, transport.conversation.Sequence = node, 4
	transport.conversation.Page.Session, transport.conversation.Page.Revision = session, 9
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "browse_files" {
			return link.Response{}, false
		}
		listing := protocol.DefaultFileBrowserListing()
		listing.Separator = protocol.Char("/")
		listing.Locations = []protocol.FileBrowserLocation{{Kind: protocol.FileBrowserLocationKindHome, Path: root}}
		listing.Directory.Path = root
		listing.Directory.Entries = []protocol.FileEntry{
			{Name: "src", Kind: protocol.EntryKindDirectory},
			{Name: "notes.md", Kind: protocol.EntryKindFile, Size: 11},
			{Name: "plan.md", Kind: protocol.EntryKindFile, Size: 10},
		}
		return link.Response{Output: protocol.Output{Kind: "file_listing", Data: listing}}, true
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	native := 0
	r.SetNativeFileActions(NativeFileActions{PickFiles: func(func([]string)) { native++ }})
	r.App.Sidebar.Selected = session.String()
	var view *ui.Context
	tester := ui.NewTesterWith(func(c *ui.Context) {
		view = c
		r.App.View(c)
	}, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	text := func(key string, args ...any) string { return i18n.In("en", key, args...) }
	settle := func() {
		for range 6 {
			r.workers.Wait()
			tester.Advance(60 * time.Millisecond)
			tester.Frame()
		}
	}
	settle()
	sent := func(kind string) (found []protocol.Request) {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		for _, request := range transport.requests {
			if request.Command.Kind == kind {
				found = append(found, request)
			}
		}
		return found
	}
	// press clicks the topmost enabled control of a role named name: the
	// picker's over the page's.
	press := func(role ui.Role, name string, mods ui.Modifiers) {
		t.Helper()
		all := tester.AccessAll()
		for i := len(all) - 1; i >= 0; i-- {
			if all[i].Role == role && all[i].Name == name && !all[i].Disabled {
				b := all[i].Bounds
				x := b.X + b.W/2
				if role == ui.RoleRow {
					x = b.X + 48 // the name's cell
				}
				tester.ClickAtWith(mods, x, b.Y+b.H/2)
				settle()
				return
			}
		}
		t.Fatalf("no enabled %q: %q", name, tester.Texts())
	}
	comp := r.App.Composer(session.String())
	cv := r.App.Data.Find(session.String())
	if cv == nil {
		t.Fatal("the session is not shown")
	}

	// The @ picker offers Attach files without a system dialog.
	ctx, _ := r.conversation(view, cv, comp)
	if ctx.Readonly || !slices.ContainsFunc(ctx.References, func(reference composer.Reference) bool { return reference.Kind == composer.AttachFiles }) {
		t.Fatalf("the @ picker has no Attach files entry: readonly %v", ctx.Readonly)
	}

	// The button opens the window's picker at the worktree, read from the
	// local Node.
	press(ui.RoleButton, text("composer_attachment"), 0)
	browsed := sent("browse_files")
	if !r.App.Sidebar.Picker.Showing() || len(browsed) == 0 || native != 0 {
		t.Fatalf("Add attachment: picker %v, %d listings, %d system dialogs", r.App.Sidebar.Picker.Showing(), len(browsed), native)
	}
	if asked, ok := detail[protocol.CommandBrowseFilesPayload](browsed[0].Command.Data); !ok || asked.Directory == nil || *asked.Directory != root {
		t.Fatalf("the picker did not open at the worktree %s: %+v", root, browsed[0].Command.Data)
	}
	for _, want := range []string{text("composer_attachment"), "notes.md", "plan.md", "src", text("directory_nothing_selected")} {
		if !slices.Contains(tester.Texts(), want) {
			t.Fatalf("the picker does not show %q: %q", want, tester.Texts())
		}
	}

	// Cancel attaches nothing.
	press(ui.RoleButton, text("settings_cancel"), 0)
	if r.App.Sidebar.Picker.Showing() || len(comp.Attachments) != 0 || len(sent("upload_attachment")) != 0 {
		t.Fatalf("Cancel: picker %v, %d attachments, %d uploads", r.App.Sidebar.Picker.Showing(), len(comp.Attachments), len(sent("upload_attachment")))
	}

	// Two files, the second added with Cmd, are both read here and uploaded
	// to the session's worktree.
	press(ui.RoleButton, text("composer_attachment"), 0)
	press(ui.RoleRow, "notes.md", 0)
	press(ui.RoleRow, "plan.md", ui.Cmd)
	if !slices.Contains(tester.Texts(), text("directory_selected_files", "count", 2)) {
		t.Fatalf("Cmd did not add to the choice: %q", tester.Texts())
	}
	press(ui.RoleButton, text("directory_open"), 0)
	for end := time.Now().Add(10 * time.Second); time.Now().Before(end) && len(sent("upload_attachment")) < 2; time.Sleep(10 * time.Millisecond) {
		settle()
	}
	var names []string
	for _, request := range sent("upload_attachment") {
		spec, ok := detail[protocol.AttachmentSpec](request.Command.Data)
		if !ok || spec.Worktree != worktree {
			t.Fatalf("an upload names another worktree: %+v", request.Command.Data)
		}
		names = append(names, spec.Name)
	}
	slices.Sort(names)
	if !slices.Equal(names, []string{"notes.md", "plan.md"}) || r.App.Sidebar.Picker.Showing() || native != 0 {
		t.Fatalf("Open uploaded %q, picker %v, %d system dialogs", names, r.App.Sidebar.Picker.Showing(), native)
	}
}

// attachmentPickerHost: an attachment is read on this machine, so the picker
// of a conversation on a paired Host lists the local Node's files, from the
// home, and a local conversation's opens at its worktree.
func attachmentPickerHost(t *testing.T) {
	localNode, remoteNode := protocol.NodeID{1}, protocol.NodeID{2}
	project, worktree, session := protocol.ProjectID{3}, protocol.WorktreeID{4}, protocol.SessionID{5}
	transports := make([]*conversationTestTransport, 2)
	owners := make([]*Runtime, 2)
	for index, node := range []protocol.NodeID{localNode, remoteNode} {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		snapshot.Projects = []protocol.Project{{ID: project, Name: "Workspace", Path: "/fixture/project"}}
		snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/worktree", Main: true}}
		snapshot.Sessions = []protocol.Session{{ID: session, Project: &project, Worktree: worktree, Revision: 7}}
		transports[index] = &conversationTestTransport{snapshot: snapshot}
		controller := client.New(transports[index])
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		defer controller.Close()
		owners[index] = &Runtime{ctx: t.Context(), controller: controller, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
	}
	local, remote := owners[0], owners[1]
	localConversation := &viewmodel.Conversation{Node: localNode.String(), ID: session.String(), Revision: 7}
	remoteConversation := &viewmodel.Conversation{Node: remoteNode.String(), ID: session.String(), Revision: 7}
	local.App = shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
	local.App.Sidebar.Actions = &sidebar.Actions{Files: local.files}
	remote.App, remote.parent = local.App, local
	local.bindings = map[string]*Runtime{remoteNode.String(): remote}
	// listings are the directories a Node was asked to list, "" for its home.
	listings := func(index int) (asked []string) {
		owners[index].workers.Wait()
		transports[index].mu.Lock()
		defer transports[index].mu.Unlock()
		for _, request := range transports[index].requests {
			if request.Command.Kind != "browse_files" {
				continue
			}
			directory := ""
			if payload, ok := detail[protocol.CommandBrowseFilesPayload](request.Command.Data); ok && payload.Directory != nil {
				directory = *payload.Directory
			}
			asked = append(asked, directory)
		}
		return asked
	}
	var open func(c *ui.Context)
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		if open != nil {
			open(c)
			open = nil
		}
	}, 640, 400)
	open = func(c *ui.Context) {
		remote.chooseAttachments(&conversationActions{r: remote, c: c, cv: remoteConversation, comp: composer.New("")})
	}
	tester.Frame()
	if got := listings(0); !slices.Equal(got, []string{""}) || len(listings(1)) != 0 || !local.App.Sidebar.Picker.Showing() {
		t.Fatalf("a remote conversation's picker listed %q locally and %q on its Host", got, listings(1))
	}
	open = func(c *ui.Context) {
		local.chooseAttachments(&conversationActions{r: local, c: c, cv: localConversation, comp: composer.New("")})
	}
	tester.Frame()
	if got := listings(0); !slices.Equal(got, []string{"", "/fixture/worktree"}) || len(listings(1)) != 0 {
		t.Fatalf("a local conversation's picker listed %q locally and %q on the Host", got, listings(1))
	}
}
