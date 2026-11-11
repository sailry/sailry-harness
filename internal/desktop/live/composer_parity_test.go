package live

import (
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// composerParityCommands drives the window over a session that runs, with a
// queue: the conversation's keys, /compact with a focus, the queue's
// commands and their outcomes, and the header's overview. It checks the
// command each sends.
func composerParityCommands(t *testing.T) {
	node, project, worktree, provider := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}, protocol.ProviderID{4}
	session, running := protocol.SessionID{5}, protocol.TurnID{6}
	plain, other := protocol.TurnID{7}, protocol.TurnID{8}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider, "fixture", "code", "ask"
	config.Effort = protocol.Effort{Kind: "default"}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Workspace", Path: "/fixture/project"}}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/worktree", Main: true}}
	snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Revision: 7, Enabled: true, Name: "Provider", Api: "responses", Authentication: "api_key", Models: []protocol.ConversationModel{{ID: "fixture", DefaultEffort: protocol.Effort{Kind: "default"}}}}}
	run := protocol.ConversationRun{Session: session, Turn: running, Worktree: worktree, Sequence: 1, Revision: 1, Kind: "task", Status: "running"}
	snapshot.Sessions = []protocol.Session{{ID: session, Revision: 6, Worktree: worktree, Project: &project, Config: config, Roles: protocol.DefaultRoleSnapshot()}}
	snapshot.Sessions[0].Activity.Title = "Parity session"
	snapshot.Sessions[0].Activity.Run = &run
	queue := protocol.ConversationQueue{Revision: 3, Items: []protocol.ConversationPending{
		{Turn: plain, Kind: "task", Revision: 1, ConfigRevision: 1, Ready: true, Preview: "Queued plain message"},
		{Turn: other, Kind: "task", Revision: 1, ConfigRevision: 1, Ready: true, Preview: "Queued second message"},
	}}
	// A Host package that declares a read-only conversation mode.
	description := "Reads and reports, changes nothing"
	modes := protocol.DefaultPluginSummary()
	modes.Name, modes.Revision, modes.Digest, modes.Enabled, modes.Description = "review-modes", 1, "digest", true, &description
	snapshot.Plugins = []protocol.PluginSummary{modes}
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.conversation = protocol.DefaultConversationSnapshot()
	transport.conversation.Node, transport.conversation.Sequence = node, 4
	transport.conversation.Page.Session, transport.conversation.Page.Revision = session, 9
	transport.conversation.Page.Runs = []protocol.ConversationRun{run}
	transport.conversation.Page.Queue = queue
	transport.conversation.Page.Entries = []protocol.ConversationEntry{{ID: "sent", Sequence: 1, Turn: running, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Run the parity checks"}}}}
	// failing fails the next command of that kind once; assetsFail every
	// inventory read while set.
	failing, assetsFail := "", false
	earlier := uint64(40)
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		kind := request.Command.Kind
		if kind == failing {
			failing = ""
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture failure"}}, true
		}
		switch kind {
		case "compact_context", "stop_turn":
			return link.Response{Output: protocol.Output{Kind: "queued_turn", Data: protocol.DefaultQueuedTurn()}}, true
		case "guide_queued_turn", "remove_queued_turn", "withdraw_guidance", "send_queued_turn":
			// The Host keeps the message queued, ready and unguided: its
			// settings differ from the running turn's.
			return link.Response{Output: protocol.Output{Kind: "queue", Data: protocol.ConversationQueueUpdate{Session: session, Queue: queue}}}, true
		case "list_conversation_assets":
			if assetsFail {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture failure"}}, true
			}
			query := request.Command.Data.(protocol.CommandListConversationAssetsPayload).Query
			page := protocol.ConversationAssetsPage{Session: session, Revision: 9}
			file := func(sequence uint64, kind protocol.ConversationAssetsKind, paths ...string) protocol.ConversationAssetsGroup {
				group := protocol.ConversationAssetsGroup{Sequence: sequence, Turn: running, Worktree: worktree, Kind: kind}
				for _, path := range paths {
					group.Items = append(group.Items, protocol.ConversationAssetsTarget{Kind: "file", Data: protocol.ConversationAssetsTargetFilePayload{Path: path}})
				}
				return group
			}
			if query.Before == nil {
				// A full page: an output, then sources, one of them twice.
				page.Groups = append(page.Groups, file(60, protocol.ConversationAssetsKindArtifact, "out/report.md"),
					file(59, protocol.ConversationAssetsKindResource, "docs/guide.md", "docs/guide.md"))
				for sequence := uint64(58); len(page.Groups) < int(query.Limit); sequence-- {
					page.Groups = append(page.Groups, file(sequence, protocol.ConversationAssetsKindResource, "docs/guide.md"))
				}
				page.NextBefore = &earlier
			} else {
				page.Groups = append(page.Groups, file(30, protocol.ConversationAssetsKindResource, "docs/earlier.md"))
			}
			return link.Response{Output: protocol.Output{Kind: "conversation_assets", Data: page}}, true
		}
		return link.Response{}, false
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
	r.App.Sidebar.Selected = session.String()
	if r.packages == nil {
		r.packages = newPackageState()
	}
	r.packages.infos[modes.Name] = protocol.PluginInfo{Summary: modes, Extension: &protocol.PluginExtension{Scope: protocol.PluginScopeHost,
		ConversationModes: []protocol.PluginModesDeclaration{{ID: "audit", Option: "style",
			Display: protocol.PluginDesktopNavigation{Label: "Audit", Locales: map[string]string{"zh-CN": "审查"}}}}}}
	// Opaque surfaces keep the software shaders out of the headless frames.
	tester := ui.NewTesterWith(r.App.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("zh-CN")
	tester.SetDark(true)
	text := func(key string, args ...any) string { return i18n.In("zh-CN", key, args...) }
	settle := func() {
		for range 6 {
			r.workers.Wait()
			tester.Advance(60 * time.Millisecond)
			tester.Frame()
		}
	}
	settle()
	shot := func(name string) {
		dir := os.Getenv("SAILRY_UI_SHOTS")
		if dir == "" {
			return
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		file, err := os.Create(filepath.Join(dir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tester.Image()); err != nil {
			t.Fatal(err)
		}
	}
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
	// press clicks the n-th control named name, top to bottom.
	press := func(name string, n int) {
		t.Helper()
		var found []ui.AccessInfo
		for _, e := range tester.AccessAll() {
			if e.Role == ui.RoleButton && e.Name == name {
				found = append(found, e)
			}
		}
		if n >= len(found) {
			t.Fatalf("%d buttons named %q, want one at %d: %q", len(found), name, n, tester.Texts())
		}
		if found[n].Disabled {
			t.Fatalf("button %q at %d is disabled", name, n)
		}
		b := found[n].Bounds
		tester.ClickAt(b.X+b.W/2, b.Y+b.H/2)
		settle()
	}
	for end := time.Now().Add(10 * time.Second); !tester.HasText("Queued plain message") && time.Now().Before(end); time.Sleep(10 * time.Millisecond) {
		settle()
	}
	if state, ok := controller.Conversation(session); !ok || !state.Connected {
		t.Logf("conversation state: ok %v, connected %v, loading %v, err %v", ok, state.Connected, state.Loading, state.Err)
	}
	if !tester.HasText("Queued plain message") {
		t.Fatalf("the session's queue is not shown: %q", tester.Texts())
	}

	// The package's mode is in the mode menu; choosing it saves the session
	// with the package's option over the read-only base mode, and Shift+Tab
	// goes on from it to the first built-in mode, clearing the option.
	tap := func(name string) {
		t.Helper()
		for _, e := range tester.AccessAll() {
			if e.Name == name && e.Role != ui.RoleText {
				tester.ClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
				settle()
				return
			}
		}
		t.Fatalf("no control %q: %q", name, tester.Texts())
	}
	tap(text("composer_mode_code"))
	if !tester.HasText("审查") || !tester.HasText(description) {
		t.Fatalf("the mode menu lacks the package's mode: %q", tester.Texts())
	}
	shot("parity-live-mode-menu")
	tap("审查")
	saves := sent("set_session_config")
	if len(saves) != 1 {
		t.Fatalf("choosing the package's mode sent %d set_session_config", len(saves))
	}
	saved := saves[0].Command.Data.(protocol.CommandSetSessionConfigPayload).Config
	if saved.Mode != protocol.WorkModePlan || string(saved.Options["review-modes"]["style"]) != `"audit"` {
		t.Fatalf("the saved mode: %s, options %s", saved.Mode, saved.Options)
	}
	settle()
	found := false
	for _, e := range tester.AccessAll() {
		found = found || e.Name == "审查" && e.Role != ui.RoleText
	}
	if !found {
		t.Errorf("the mode control does not name the package's mode: %q", tester.Texts())
	}
	if err := tester.Click(text("composer")); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Shift, ui.KeyTab)
	settle()
	saves = sent("set_session_config")
	if len(saves) != 2 {
		t.Fatalf("Shift+Tab sent %d set_session_config in all", len(saves))
	}
	saved = saves[1].Command.Data.(protocol.CommandSetSessionConfigPayload).Config
	if saved.Mode != protocol.WorkModeCode || string(saved.Options["review-modes"]["style"]) != `""` {
		t.Fatalf("after Shift+Tab: %s, options %s", saved.Mode, saved.Options)
	}

	// /compact with a focus compacts with it, and sends no message.
	if err := tester.Click(text("composer")); err != nil {
		t.Fatal(err)
	}
	tester.Type("/compact keep the API notes")
	tester.Frame()
	tester.Key(0, ui.KeyEnter)
	settle()
	compacts := sent("compact_context")
	if len(compacts) != 1 {
		t.Fatalf("/compact sent %d compact_context", len(compacts))
	}
	if payload := compacts[0].Command.Data.(protocol.CommandCompactContextPayload); payload.Focus == nil || *payload.Focus != "keep the API notes" || payload.Session != session {
		t.Fatalf("compact_context payload %+v", payload)
	}
	if len(sent("submit_turn"))+len(sent("guide_turn")) != 0 || r.App.Composer(session.String()).Draft != "" {
		t.Fatalf("/compact was sent as a message or left its draft %q", r.App.Composer(session.String()).Draft)
	}

	// Send now on a queued row guides the running turn with it; the Host
	// left it queued, which the user is told.
	press(text("queue_send_now"), 0)
	guides := sent("guide_queued_turn")
	if len(guides) != 1 {
		t.Fatalf("Send now sent %d guide_queued_turn", len(guides))
	}
	if payload := guides[0].Command.Data.(protocol.CommandGuideQueuedTurnPayload); payload.Turn != plain || payload.Recipient != running || payload.ExpectedRevision != 1 {
		t.Fatalf("guide_queued_turn payload %+v", payload)
	}
	if !tester.HasText(text("queue_guidance_separate")) {
		t.Errorf("no 'sent separately' toast: %q", tester.Texts())
	}
	shot("parity-live-queue-separate")

	// A failed queue command offers Retry, which sends it again.
	failing = "remove_queued_turn"
	press(text("queue_delete"), 1)
	if len(sent("remove_queued_turn")) != 1 || !tester.HasText(text("chat_action_failed")) {
		t.Fatalf("failed Remove: %d sent, texts %q", len(sent("remove_queued_turn")), tester.Texts())
	}
	shot("parity-live-queue-retry")
	press(text("chat_retry"), 0)
	removes := sent("remove_queued_turn")
	if len(removes) != 2 {
		t.Fatalf("Retry sent %d remove_queued_turn in all", len(removes))
	}
	if payload := removes[1].Command.Data.(protocol.CommandRemoveQueuedTurnPayload); payload.Turn != other {
		t.Fatalf("retried remove_queued_turn payload %+v", payload)
	}

	// The header's overview reads the inventory when it opens.
	if len(sent("list_conversation_assets")) != 0 {
		t.Fatal("the inventory was read before the overview opened")
	}
	press(text("chat_assets_title"), 0)
	reads := sent("list_conversation_assets")
	if len(reads) != 1 {
		t.Fatalf("opening the overview sent %d list_conversation_assets", len(reads))
	}
	if payload := reads[0].Command.Data.(protocol.CommandListConversationAssetsPayload); payload.Session != session || payload.Query.Limit != overviewLimit || payload.Query.Before != nil {
		t.Fatalf("list_conversation_assets payload %+v", payload)
	}
	for _, want := range []string{text("chat_assets_activity"), text("chat_assets_no_activity"), text("chat_assets_output"), "out/report.md", text("chat_assets_sources"), "guide.md", text("chat_assets_more")} {
		if !tester.HasText(want) {
			t.Fatalf("the overview lacks %q: %q", want, tester.Texts())
		}
	}
	rows := 0
	for _, e := range tester.AccessAll() {
		if e.Role == ui.RoleButton && e.Name == "docs/guide.md" {
			rows++
		}
	}
	if rows != 1 {
		t.Errorf("a source listed twice shows %d rows", rows)
	}
	shot("parity-live-overview")
	// Show earlier reads the page before the last group.
	press(text("chat_assets_more"), 0)
	reads = sent("list_conversation_assets")
	if len(reads) != 2 {
		t.Fatalf("Show earlier sent %d list_conversation_assets in all", len(reads))
	}
	if payload := reads[1].Command.Data.(protocol.CommandListConversationAssetsPayload); payload.Query.Before == nil || *payload.Query.Before != earlier {
		t.Fatalf("the earlier page's query %+v", payload.Query)
	}
	if !tester.HasText("earlier.md") || !tester.HasText("guide.md") || tester.HasText(text("chat_assets_more")) {
		t.Fatalf("the earlier page: %q", tester.Texts())
	}
	// Closed and opened again with nothing new, it reads nothing.
	tester.Key(0, ui.KeyEscape)
	settle()
	press(text("chat_assets_title"), 0)
	if len(sent("list_conversation_assets")) != 2 {
		t.Errorf("reopening a current overview read the inventory again: %d reads", len(sent("list_conversation_assets")))
	}
	tester.Key(0, ui.KeyEscape)
	settle()
	// A read that fails says so and offers Retry.
	state := r.overviews[session]
	state.loaded, state.groups, state.before = false, nil, nil
	assetsFail = true
	press(text("chat_assets_title"), 0)
	if !tester.HasText(text("chat_assets_failed")) {
		t.Fatalf("no failure toast: %q", tester.Texts())
	}
	assetsFail = false
	before := len(sent("list_conversation_assets"))
	var retry ui.AccessInfo
	for _, e := range tester.AccessAll() {
		if e.Role == ui.RoleButton && e.Name == text("chat_retry") {
			retry = e // the popover's, drawn after the toast's
		}
	}
	if retry.Name == "" {
		t.Fatalf("no Retry in the failed overview: %q", tester.Texts())
	}
	shot("parity-live-overview-failed")
	tester.ClickAt(retry.Bounds.X+retry.Bounds.W/2, retry.Bounds.Y+retry.Bounds.H/2)
	settle()
	if len(sent("list_conversation_assets")) != before+1 || !tester.HasText("out/report.md") {
		t.Fatalf("Retry: %d reads, texts %q", len(sent("list_conversation_assets"))-before, tester.Texts())
	}
	tester.Key(0, ui.KeyEscape)
	settle()

	// ⌘L focuses the composer, ⇧⌘M opens the model controls and ⌘. stops
	// the running turn.
	tester.Key(ui.Cmd, ui.KeyL)
	settle()
	if !tester.Focused(text("composer")) {
		t.Error("⌘L did not focus the composer")
	}
	if tester.HasText(text("composer_effort")) {
		t.Fatal("the model controls are open before ⇧⌘M")
	}
	tester.Key(ui.Cmd|ui.Shift, ui.KeyM)
	settle()
	if !tester.HasText(text("composer_effort")) {
		t.Errorf("⇧⌘M did not open the model controls: %q", tester.Texts())
	}
	tester.Key(0, ui.KeyEscape)
	settle()
	tester.Key(ui.Cmd, ui.KeyPeriod)
	settle()
	stops := sent("stop_turn")
	if len(stops) != 1 {
		t.Fatalf("⌘. sent %d stop_turn", len(stops))
	}
	if payload := stops[0].Command.Data.(protocol.CommandStopTurnPayload); payload.Turn != running {
		t.Fatalf("stop_turn payload %+v", payload)
	}

	// A message on its way to the queue is the tray's last row until the
	// Host answers.
	comp := r.App.Composer(session.String())
	cv := r.App.Data.Find(session.String())
	var view *ui.Context
	probe := ui.NewTester(func(c *ui.Context) { view = c }, 64, 64)
	probe.Frame()
	actions := &conversationActions{r: r, c: view, cv: cv, comp: comp}
	actions.outgoing(composer.Message{Text: "  Outgoing message  "}, true)
	settle()
	if !tester.HasText("Outgoing message") {
		t.Errorf("no outgoing row: %q", tester.Texts())
	}
	delete(r.composerOutgoing, cv.ID)

	// A stale command or skill is one the conversation offers no more.
	offered := &composer.Context{Commands: []composer.Reference{{Source: "plugin-contribution", ID: "kept", Kind: composer.Prompt}},
		References: []composer.Reference{{ID: "skills", Kind: composer.Page, Children: []composer.Reference{{ID: "pack:skill", Kind: composer.Skill}}}}}
	for _, row := range []struct {
		reference composer.Reference
		stale     bool
	}{
		{composer.Reference{Source: "plugin-contribution", ID: "kept", Kind: composer.Prompt}, false},
		{composer.Reference{Source: "plugin-contribution", ID: "gone", Kind: composer.Prompt}, true},
		{composer.Reference{ID: "pack:skill", Kind: composer.Skill}, false},
		{composer.Reference{ID: "pack:removed", Kind: composer.Skill}, true},
		{composer.Reference{ID: "prompt:review", Kind: composer.Prompt}, false},
	} {
		if got := staleReference(offered, row.reference); got != row.stale {
			t.Errorf("staleReference(%s %s) = %v", row.reference.Source, row.reference.ID, got)
		}
	}
	if strings.TrimSpace(comp.Draft) != "" {
		t.Errorf("the steps left a draft: %q", comp.Draft)
	}
	t.Run("child session tray", childTray)
}

// childTray shows a subagent's session with a queued message: no composer,
// a read-only tray whose Read reads the message and whose Remove removes it.
func childTray(t *testing.T) {
	node, worktree, parent, child, queued := protocol.NodeID{1}, protocol.WorktreeID{3}, protocol.SessionID{5}, protocol.SessionID{6}, protocol.TurnID{7}
	origin := &protocol.Delegation{Session: parent, Turn: protocol.TurnID{1}, Entry: "spawn"}
	config := protocol.DefaultSessionConfig()
	config.Mode, config.Permission, config.Model = protocol.WorkModeCode, protocol.PermissionAsk, "fixture"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Main: true}}
	snapshot.Sessions = []protocol.Session{{ID: child, Revision: 1, Worktree: worktree, Config: config, Roles: protocol.DefaultRoleSnapshot(), Delegation: &origin}}
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.conversation = protocol.DefaultConversationSnapshot()
	transport.conversation.Node = node
	transport.conversation.Page.Session, transport.conversation.Page.Revision = child, 2
	transport.conversation.Page.Queue = protocol.ConversationQueue{Revision: 1, Items: []protocol.ConversationPending{
		{Turn: queued, Kind: "task", Revision: 4, ConfigRevision: 1, Ready: true, Preview: "Guidance for the subagent"}}}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_queued_turn":
			message := protocol.DefaultConversationQueuedMessage()
			message.Turn.ID, message.Revision, message.Message.Text = queued, 4, "Guidance for the subagent, in full"
			return link.Response{Output: protocol.Output{Kind: "queued_message", Data: message}}, true
		case "remove_queued_turn":
			return link.Response{Output: protocol.Output{Kind: "queue", Data: protocol.ConversationQueueUpdate{Session: child, Queue: transport.conversation.Page.Queue}}}, true
		}
		return link.Response{}, false
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
	cv := &viewmodel.Conversation{Node: node.String(), ID: child.String()}
	comp := r.App.Composer(cv.ID)
	var shown *composer.Context
	tester := ui.NewTesterWith(func(c *ui.Context) {
		r.drain(c)
		ctx, actions := r.conversation(c, cv, comp)
		shown = ctx
		ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, comp, ctx, actions) })
		r.finishFrame()
	}, 960, 520, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("zh-CN")
	tester.SetDark(true)
	text := func(key string) string { return i18n.In("zh-CN", key) }
	settle := func() {
		for range 6 {
			r.workers.Wait()
			tester.Advance(60 * time.Millisecond)
			tester.Frame()
		}
	}
	for end := time.Now().Add(10 * time.Second); !tester.HasText("Guidance for the subagent") && time.Now().Before(end); time.Sleep(10 * time.Millisecond) {
		settle()
	}
	if !shown.Child || !shown.Readonly {
		t.Fatalf("a subagent's session: child %v, read-only %v", shown.Child, shown.Readonly)
	}
	if _, ok := tester.Access(text("composer")); ok {
		t.Error("a subagent's session shows the composer's input")
	}
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
	press := func(name string) {
		t.Helper()
		var target ui.AccessInfo
		for _, e := range tester.AccessAll() {
			if e.Role == ui.RoleButton && e.Name == name {
				target = e
			}
		}
		if target.Name == "" || target.Disabled {
			t.Fatalf("no enabled button %q: %q", name, tester.Texts())
		}
		tester.ClickAt(target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2)
		settle()
	}
	for _, e := range tester.AccessAll() {
		if e.Role == ui.RoleButton && (e.Name == text("queue_send_now") || e.Name == text("queue_edit")) {
			t.Errorf("a subagent's tray offers %q", e.Name)
		}
	}
	press(text("queue_read"))
	if reads := sent("read_queued_turn"); len(reads) != 1 || reads[0].Command.Data.(protocol.CommandReadQueuedTurnPayload).Turn != queued {
		t.Fatalf("Read sent %+v", reads)
	}
	// The read-only editor holds the whole message: Copy takes it.
	press(text("message_copy"))
	if tester.Clipboard() != "Guidance for the subagent, in full" {
		t.Errorf("Copy wrote %q", tester.Clipboard())
	}
	press(text("queue_delete"))
	removes := sent("remove_queued_turn")
	if len(removes) != 1 {
		t.Fatalf("Remove sent %d remove_queued_turn", len(removes))
	}
	if payload := removes[0].Command.Data.(protocol.CommandRemoveQueuedTurnPayload); payload.Turn != queued || payload.ExpectedRevision != 4 {
		t.Fatalf("remove_queued_turn payload %+v", payload)
	}
	if len(sent("edit_queued_turn"))+len(sent("send_queued_turn"))+len(sent("guide_queued_turn")) != 0 {
		t.Error("a subagent's tray sent or edited a message")
	}
}
