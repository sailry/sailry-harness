package live

import (
	"context"
	"errors"
	"image/png"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
	"unicode"

	"github.com/egoist/mygo/ui"
	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// firstRunTransport records the kind of every command the desktop sends. A
// kind in hold is not forwarded: it waits for its gate and then fails.
type firstRunTransport struct {
	link.Transport
	mu    *sync.Mutex
	kinds *[]string
	hold  map[string]chan struct{}
	// gate, when set, keeps create_project from the Node until it is closed.
	gate *atomic.Pointer[chan struct{}]
}

func (t firstRunTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	name := request.Command.Kind
	if request.Plugin != nil {
		name = request.Plugin.Package.Name + ":" + name
	}
	t.mu.Lock()
	*t.kinds = append(*t.kinds, name)
	t.mu.Unlock()
	if gate := t.hold[name]; gate != nil {
		select {
		case <-gate:
		case <-ctx.Done():
		}
		return link.Admission{}, errors.New("held command released")
	}
	if t.gate != nil && name == "create_project" {
		if gate := t.gate.Load(); gate != nil {
			<-*gate
		}
	}
	return t.Transport.Dispatch(ctx, request)
}

// firstRun drives the whole desktop against a real Node on a new profile
// without a provider. It runs as a step of TestComposerSendRouting.
func firstRun(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 6*time.Minute)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.StartWithOptions(ctx, profile, node.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	var mu sync.Mutex
	var kinds []string
	// The catalog refresh would ask the network: it is held, then fails.
	refresh := make(chan struct{})
	var gate atomic.Pointer[chan struct{}]
	controller := client.New(firstRunTransport{Transport: link.NewLocal(owner.ID(), owner.ID(), owner), mu: &mu, kinds: &kinds,
		hold: map[string]chan struct{}{"refresh_model_catalog": refresh}, gate: &gate})
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(ctx, controller, preferences.ForStartup(profile))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	r.App.NavOpen = true
	var asked atomic.Bool
	r.SetUpdate(func(func()) { asked.Store(true) })
	var view *ui.Context
	tester := ui.NewTester(func(c *ui.Context) {
		view = c
		r.App.View(c)
	}, 1280, 820)
	tester.SetLocale("en")
	text := func(key string) string { return i18n.In("en", key) }
	sent := func() []string {
		mu.Lock()
		defer mu.Unlock()
		return append([]string(nil), kinds...)
	}
	count := func(kind string) int {
		total := 0
		for _, name := range sent() {
			if name == kind {
				total++
			}
		}
		return total
	}
	// run shows the window for a while the way the display would: a frame
	// only when one was asked for. It returns the frames drawn.
	run := func(d time.Duration) int {
		frames := 0
		for end := time.Now().Add(d); time.Now().Before(end); time.Sleep(5 * time.Millisecond) {
			tester.Advance(16 * time.Millisecond)
			if asked.Swap(false) || tester.FramePending() {
				tester.Frame()
				frames++
			}
		}
		return frames
	}
	settle := func() {
		for range 4 {
			r.workers.Wait()
			tester.Frame()
		}
		run(300 * time.Millisecond)
	}
	// idle lets what is in flight end on the window's own clock, then shows
	// the window for a second and returns the frames it drew. The toasts and
	// their motion run on that clock, which run moves by how many frames the
	// machine draws meanwhile: left to it, a toast still arriving or just
	// leaving counts as redraws of an idle page. Eight seconds, a refresh at
	// a time, are past a toast's five and its motion; a view that never
	// settles still asks for frames afterwards.
	idle := func() int {
		settle()
		for range 480 {
			tester.Advance(17 * time.Millisecond)
		}
		settle()
		return run(time.Second)
	}
	listed := func(id string) bool {
		for _, shown := range r.App.Sidebar.Order() {
			if shown == id {
				return true
			}
		}
		return r.App.Sidebar.Data.Find(id) != nil
	}
	settle()
	if tester.HasText(text("close")) {
		if err := tester.Click(text("close")); err != nil {
			t.Fatal(err)
		}
		settle()
	}

	// The window opens on an unsaved draft: no Node session, no sidebar row.
	draft := r.App.Sidebar.Selected
	if !strings.HasPrefix(draft, "draft/") || r.App.Page != shell.ConversationPage || r.App.Data.Find(draft) == nil {
		t.Fatalf("a new profile did not open on a draft: %q", draft)
	}
	if listed(draft) || len(r.App.Sidebar.Order()) != 0 {
		t.Fatalf("the unsent draft is listed in the sidebar: %v", r.App.Sidebar.Order())
	}
	if len(controller.Snapshot().Sessions) != 0 || count("create_session") != 0 {
		t.Fatal("opening the window created a Node session")
	}

	// Idle: no frame is asked for and no command is sent.
	before := len(sent())
	if frames := run(1500 * time.Millisecond); frames > 2 {
		t.Fatalf("the idle window redrew %d times", frames)
	}
	if idle := sent()[before:]; len(idle) != 0 {
		t.Fatalf("the idle window sent commands: %v", idle)
	}

	// New session over the draft keeps that one draft and creates nothing.
	if err := tester.Click(text("conversation")); err != nil {
		t.Fatal(err)
	}
	settle()
	if r.App.Sidebar.Selected != draft || len(r.App.Data.Unassigned) != 1 || count("create_session") != 0 || len(controller.Snapshot().Sessions) != 0 {
		t.Fatalf("New session stacked a draft or created a session: selected %q, %d conversations", r.App.Sidebar.Selected, len(r.App.Data.Unassigned))
	}

	// Without a usable model Send is not offered, and input stays a draft.
	comp := r.App.Composer(draft)
	cv := r.App.Data.Find(draft)
	blocked, _ := r.conversation(view, cv, comp)
	if !blocked.SendBlocked || blocked.SelectModel == nil {
		t.Fatal("a draft without a configured model offers Send")
	}
	if err := tester.Click(text("composer")); err != nil {
		t.Fatal(err)
	}
	tester.Type("hello")
	tester.Frame()
	if comp.Draft != "hello" {
		t.Fatalf("typing did not reach the draft: %q", comp.Draft)
	}
	before = len(sent())
	_ = tester.Click(text("send"))
	settle()
	if comp.Draft != "hello" || len(sent()) != before {
		t.Fatalf("the blocked Send acted: draft %q, commands %v", comp.Draft, sent()[before:])
	}
	// The command itself, reached any other way, fails as a normal fault.
	r.enqueue(func(c *ui.Context) {
		actions := &conversationActions{r: r, c: c, cv: cv, comp: comp}
		actions.Send(composer.Message{Text: comp.Draft, Mode: comp.Mode, Permission: comp.Permission})
	})
	settle()
	if comp.Draft != "hello" || !tester.HasText(text("chat_config_unavailable")) {
		t.Fatalf("an unconfigured send did not keep the draft and toast: draft %q", comp.Draft)
	}
	if count("create_session") != 0 || count("submit_turn") != 0 || len(controller.Snapshot().Sessions) != 0 || r.App.Sidebar.Selected != draft {
		t.Fatal("an unconfigured send reached the Node")
	}
	// The Node rejects an unconfigured creation and keeps answering.
	rejected := make(chan error, 1)
	r.execute(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &protocol.SessionConfig{}}}, func(_ *ui.Context, _ protocol.Output, err error) {
		rejected <- err
	})
	settle()
	select {
	case err := <-rejected:
		if err == nil || len(controller.Snapshot().Sessions) != 0 {
			t.Fatalf("the Node created a session without a model: %v", err)
		}
	default:
		t.Fatal("the Node did not answer an unconfigured creation")
	}
	second := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := second.Start(ctx); err != nil {
		t.Fatal("the Node stopped answering after the failed send:", err)
	}
	second.Close()

	// The conversation page, its draft focused, redraws nothing while idle.
	before = len(sent())
	if frames := idle(); frames != 0 || len(sent()) != before || r.App.Page != shell.ConversationPage {
		t.Fatalf("the conversation page redrew %d times and sent %v while idle", frames, sent()[before:])
	}

	// A draft with input survives navigation; an untouched one leaves no trace.
	r.App.Navigate(shell.SettingsPage)
	settle()
	if r.App.Data.Find(draft) == nil || comp.Draft != "hello" {
		t.Fatal("navigating away dropped a draft with input")
	}
	comp.Draft = ""
	r.App.Sidebar.Selected = ""
	settle()
	if r.App.Data.Find(draft) != nil || len(r.App.Data.Unassigned) != 0 || len(controller.Snapshot().Sessions) != 0 {
		t.Fatal("an untouched draft was kept after navigating away")
	}
	before = len(sent())
	if frames := idle(); frames != 0 || len(sent()) != before {
		t.Fatalf("the settings page redrew %d times and sent %v while idle", frames, sent()[before:])
	}
	firstRunChinese(t, r, tester, func() *ui.Context { return view }, settle, sent, &gate)

	// The Chinese pass ends on a conversation; Update is on the providers'
	// settings.
	r.App.Settings.Open(settings.Providers)
	r.App.Navigate(shell.SettingsPage)
	settle()

	// Model data's Update is busy from the press until the Node answers: a
	// second press sends nothing, and the answer gives the button back.
	update := func() ui.AccessInfo {
		t.Helper()
		info, ok := tester.AccessByRole(ui.RoleButton, i18n.In("zh-CN", "provider_catalog_update"))
		if !ok {
			t.Fatalf("no Update button: %q", tester.Texts())
		}
		return info
	}
	press := func() {
		info := update()
		tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		tester.Frame()
		run(200 * time.Millisecond)
	}
	if info := update(); info.Busy || info.Disabled {
		t.Fatalf("Update is busy before a press: %+v", info)
	}
	press()
	if info := update(); !info.Busy || !info.Disabled || count("refresh_model_catalog") != 1 {
		t.Fatalf("the pressed Update is %+v after %d refreshes", info, count("refresh_model_catalog"))
	}
	press()
	if count("refresh_model_catalog") != 1 {
		t.Fatalf("the busy Update sent %d refreshes", count("refresh_model_catalog"))
	}
	close(refresh)
	settle()
	if info := update(); info.Busy || info.Disabled || count("refresh_model_catalog") != 1 {
		t.Fatalf("the finished Update is %+v after %d refreshes", info, count("refresh_model_catalog"))
	}
}

// firstRunChinese shows the first-run window in Chinese: the draft without a
// ring, the header's buttons, New session's refocus, a plugin page entered
// once, and no English where the catalog has Chinese. It is a step of
// firstRun; SAILRY_UI_SHOTS names a directory that receives its frames.
func firstRunChinese(t *testing.T, r *Runtime, tester *ui.Tester, view func() *ui.Context, settle func(), sent func() []string, hold *atomic.Pointer[chan struct{}]) {
	// shots saves the frame in the light and the dark theme.
	shots := func(name string) {
		dir := os.Getenv("SAILRY_UI_SHOTS")
		if dir == "" {
			return
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		for _, dark := range []bool{false, true} {
			tester.SetDark(dark)
			settle()
			suffix := "-light"
			if dark {
				suffix = "-dark"
			}
			file, err := os.Create(filepath.Join(dir, name+"-zh-CN"+suffix+".png"))
			if err != nil {
				t.Fatal(err)
			}
			err = png.Encode(file, tester.Image())
			file.Close()
			if err != nil {
				t.Fatal(err)
			}
		}
		tester.SetDark(false)
		settle()
	}
	zh := func(key string) string { return i18n.In("zh-CN", key) }
	// english are the catalog's English texts that have a Chinese one.
	english := map[string]string{}
	for _, key := range i18n.Keys("en") {
		if en := i18n.In("en", key); en != zh(key) {
			english[en] = key
		}
	}
	// translated fails for a text on screen the catalog has in Chinese, and
	// with known for a Latin-only text that is not one of those names.
	translated := func(page string, known ...string) {
		t.Helper()
		for _, shown := range tester.Texts() {
			if key, exists := english[shown]; exists {
				t.Errorf("%s shows %q in English although %s is translated", page, shown, key)
			}
			if known == nil || strings.TrimSpace(shown) == "" || slices.Contains(known, shown) {
				continue
			}
			if !strings.ContainsFunc(shown, func(r rune) bool { return r > unicode.MaxASCII }) && strings.ContainsFunc(shown, unicode.IsLetter) {
				t.Errorf("%s shows the untranslated %q", page, shown)
			}
		}
	}
	button := func(name string) ui.AccessInfo {
		t.Helper()
		info, ok := tester.AccessByRole(ui.RoleButton, name)
		if !ok {
			t.Fatalf("no %q button: %q", name, tester.Texts())
		}
		return info
	}
	click := func(info ui.AccessInfo) {
		tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		settle()
	}

	tester.SetLocale("zh-CN")
	r.App.Navigate(shell.ConversationPage)
	r.App.Sidebar.NewChat()
	settle()
	settle()
	// The toast of the failed send was made in English before the switch.
	if _, shown := tester.AccessByRole(ui.RoleButton, zh("close")); shown {
		click(button(zh("close")))
	}
	draft := r.App.Sidebar.Selected
	if !strings.HasPrefix(draft, "draft/") {
		t.Fatalf("New session did not open a draft: %q", draft)
	}
	// The plugins' own names stay as they are; everything else is Chinese.
	// The location control read Chats while packages were mounted in English.
	translated("the first-run conversation", "Sailry", "SSH", "Git")
	if tester.HasText("Chats") || !tester.HasText(zh("composer_no_project")) {
		t.Errorf("the context bar does not read %q: %q", zh("composer_no_project"), tester.Texts())
	}

	// A draft has no session and so no context ring.
	cv, comp := r.App.Data.Find(draft), r.App.Composer(draft)
	if projected, _ := r.conversation(view(), cv, comp); len(projected.Indicators) != 0 {
		t.Errorf("a draft shows %d context rings", len(projected.Indicators))
	}

	// Every button of the header, the plugins' as the shell's own, is the
	// same box on one line, the ones side by side one gap apart.
	var header []ui.AccessInfo
	for _, info := range tester.AccessAll() {
		if info.Role == ui.RoleButton && info.Bounds.Y+info.Bounds.H <= shell.HeaderHeight {
			header = append(header, info)
		}
	}
	names := func() (all []string) {
		for _, info := range header {
			all = append(all, info.Name)
		}
		return
	}
	statistics := "上下文" // the statistics package's header control
	if !slices.Contains(names(), statistics) || !slices.Contains(names(), zh("details")) || len(header) < 3 {
		t.Fatalf("the header's buttons are %q", names())
	}
	for index, info := range header {
		if info.Bounds.W != shell.HeaderButton || info.Bounds.H != shell.HeaderButton || info.Bounds.Y != header[0].Bounds.Y {
			t.Errorf("header button %q is %v, want %v square at %v", info.Name, info.Bounds, shell.HeaderButton, header[0].Bounds.Y)
		}
		// The title lies between the navigation's toggle and the rest.
		if index > 0 && header[index-1].Name != zh("sidebar_toggle") {
			if gap := info.Bounds.X - (header[index-1].Bounds.X + header[index-1].Bounds.W); gap != shell.ButtonGap {
				t.Errorf("%q starts %v after %q, want %v", info.Name, gap, header[index-1].Name, shell.ButtonGap)
			}
		}
	}
	// The statistics package's control is the window's to open: a press
	// shows its panel and leaves the button enabled, a second one closes it.
	click(button(statistics))
	if opened := button(statistics); !opened.Expanded || opened.Disabled {
		t.Errorf("a press on %q left it expanded %v, disabled %v", statistics, opened.Expanded, opened.Disabled)
	}
	shots("first-run-context-panel")
	click(button(statistics))
	if closed := button(statistics); closed.Expanded || closed.Disabled {
		t.Errorf("a second press on %q left it expanded %v, disabled %v", statistics, closed.Expanded, closed.Disabled)
	}
	shots("first-run-draft")

	// New session over the draft stays enabled and puts the caret back.
	details := button(zh("details"))
	click(details)
	if tester.Focused(zh("composer")) {
		t.Fatal("the editor kept the focus when the details opened")
	}
	again := button(zh("conversation"))
	if again.Disabled {
		t.Error("New session is disabled over the draft")
	}
	click(again)
	if r.App.Sidebar.Selected != draft || len(r.App.Data.Unassigned) != 1 || !tester.Focused(zh("composer")) {
		t.Errorf("New session over the draft: selected %q of %d, editor focused %v", r.App.Sidebar.Selected, len(r.App.Data.Unassigned), tester.Focused(zh("composer")))
	}
	click(button(zh("details")))

	// Entering a plugin page reads its view and loads its data once.
	before := len(sent())
	scheduled := "定时任务" // the scheduled-tasks package's rail entry
	click(button(scheduled))
	settle()
	entry := map[string]int{}
	for _, kind := range sent()[before:] {
		entry[kind]++
	}
	for _, kind := range []string{"read_plugin_view", "scheduled-tasks:list_plugin_keys", "scheduled-tasks:read_project_catalog", "scheduled-tasks:list_plugin_models", "scheduled-tasks:list_teams"} {
		if entry[kind] != 1 {
			t.Errorf("entering the page sent %s %d times: %q", kind, entry[kind], sent()[before:])
		}
	}
	translated("the scheduled tasks page", "Sailry", "SSH", "Git")
	shots("scheduled-tasks")

	// The providers' settings of a profile without one.
	r.App.Settings.Open(settings.Providers)
	r.App.Navigate(shell.SettingsPage)
	settle()
	settle()
	if !tester.HasText(zh("provider_empty_live")) {
		t.Errorf("the providers' settings do not say %q: %q", zh("provider_empty_live"), tester.Texts())
	}
	translated("the providers' settings")
	// Tall enough for the empty list under the model data.
	tester.SetSize(1280, 1100)
	shots("settings-providers-empty")
	tester.SetSize(1280, 820)
	firstRunConnectors(t, r, tester, settle, sent, shots)
	firstRunLocation(t, r, tester, settle, shots)
	firstRunProjects(t, r, tester, settle, sent, shots, hold)
}

// firstRunProjects adds, edits and removes a project through the editor
// against the real Node: the appearance reaches the Node and the sidebar, the
// in-app picker lists a real directory tree through browse_files, Save is
// pending while the Node works, and a second real Node bound as a remote
// Host is browsed through its own controller. It is a step of firstRun.
func firstRunProjects(t *testing.T, r *Runtime, tester *ui.Tester, settle func(), sent func() []string, shots func(string), hold *atomic.Pointer[chan struct{}]) {
	zh := func(key string, a ...any) string { return i18n.In("zh-CN", key, a...) }
	count := func(kind string) (n int) {
		for _, name := range sent() {
			if name == kind {
				n++
			}
		}
		return
	}
	// press clicks the topmost button of a name: the dialog's over the page's.
	press := func(name string) {
		t.Helper()
		all := tester.AccessAll()
		for i := len(all) - 1; i >= 0; i-- {
			if all[i].Role == ui.RoleButton && all[i].Name == name {
				tester.ClickAt(all[i].Bounds.X+all[i].Bounds.W/2, all[i].Bounds.Y+all[i].Bounds.H/2)
				settle()
				return
			}
		}
		t.Fatalf("no %q button: %q", name, tester.Texts())
	}
	field := func(name string) {
		t.Helper()
		info, ok := tester.AccessByRole(ui.RoleTextField, name)
		if !ok {
			t.Fatalf("no %q field: %q", name, tester.Texts())
		}
		tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		settle()
	}
	stored := func(name string) (protocol.Project, bool) {
		for _, project := range r.controller.Snapshot().Projects {
			if project.Name == name {
				return project, true
			}
		}
		return protocol.Project{}, false
	}
	shown := func(name string) *viewmodel.Project {
		for _, project := range r.App.Data.Projects {
			if project.Name == name {
				return project
			}
		}
		return nil
	}
	// menu opens the native menu of a project's sidebar row; the project's
	// overview shows its name too.
	menu := func(name string) {
		t.Helper()
		for _, info := range tester.AccessAll() {
			if b := info.Bounds; info.Name == name && b.X >= shell.RailWidth && b.X+b.W <= shell.RailWidth+shell.NavWidth {
				tester.RightClickAt(b.X+b.W/2, b.Y+b.H/2)
				settle()
				return
			}
		}
		t.Fatalf("no sidebar row %q: %q", name, tester.Texts())
	}
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	for _, dir := range []string{"alpha", filepath.Join("beta", "inner"), ".secret"} {
		if err := os.MkdirAll(filepath.Join(root, dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(root, "notes.md"), []byte("# notes\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	// The editor: the appearance, then the folder through the picker.
	press(zh("project_add"))
	if !tester.HasText(zh("project_local")) || !tester.HasText(zh("project_clone")) {
		t.Fatalf("the live editor does not offer Local and Clone: %q", tester.Texts())
	}
	press(zh("project_appearance"))
	press(zh("project_color_blue"))
	press(zh("project_icon_terminal"))
	shots("project-editor-appearance")
	tester.Key(0, ui.KeyEscape)
	settle()
	if draft := r.App.Sidebar.Draft(); draft.Icon != "terminal" || draft.Color != "blue" || !draft.Open {
		t.Fatalf("the draft's appearance is %q %q", draft.Icon, draft.Color)
	}
	field(zh("project_path"))
	tester.Type(root)
	settle()
	before := count("browse_files")
	press(zh("directory_title"))
	settle()
	if count("browse_files") == before || !r.App.Sidebar.Picker.Showing() {
		t.Fatal("the folder button did not browse the Node")
	}
	for _, name := range []string{"alpha", "beta", "notes.md", zh("directory_count_directories", "count", 2), zh("directory_count_hidden", "count", 1)} {
		if !tester.HasText(name) {
			t.Fatalf("the picker does not list %q of %s: %q", name, root, tester.Texts())
		}
	}
	if tester.HasText(".secret") {
		t.Fatal("the picker shows a hidden directory before the toggle")
	}
	field(zh("directory_filter"))
	tester.Type("be")
	settle()
	if tester.HasText("alpha") || tester.HasText("notes.md") || !tester.HasText("beta") {
		t.Fatalf("the filter did not narrow the entries: %q", tester.Texts())
	}
	press(zh("directory_filter")) // the field's clear button
	// The hidden-files toggle is an item of the header's More menu.
	choose := func(name string, roles ...ui.Role) {
		t.Helper()
		all := tester.AccessAll()
		for i := len(all) - 1; i >= 0; i-- {
			if all[i].Name == name && slices.Contains(roles, all[i].Role) {
				tester.ClickAt(all[i].Bounds.X+all[i].Bounds.W/2, all[i].Bounds.Y+all[i].Bounds.H/2)
				settle()
				return
			}
		}
		t.Fatalf("no %q in the picker's menu: %q", name, tester.Texts())
	}
	choose(zh("more"), ui.RoleMenuButton, ui.RoleButton)
	choose(zh("directory_hidden"), ui.RoleMenuItem, ui.RoleMenuItemCheckBox, ui.RoleMenuItemRadio)
	if !tester.HasText(".secret") || !tester.HasText("alpha") || r.Preferences.Data.FilePicker == nil || !r.Preferences.Data.FilePicker.Hidden {
		t.Fatalf("the hidden toggle did not show and save: %q", tester.Texts())
	}
	shots("project-picker")
	if err := tester.Click("beta"); err != nil {
		t.Fatal(err)
	}
	settle()
	tester.Key(0, ui.KeyEnter)
	settle()
	if !tester.HasText("inner") || tester.HasText("alpha") {
		t.Fatalf("Enter did not open the directory: %q", tester.Texts())
	}
	if err := tester.Click("inner"); err != nil {
		t.Fatal(err)
	}
	settle()
	press(zh("directory_title"))
	chosen := filepath.Join(root, "beta", "inner")
	if draft := r.App.Sidebar.Draft(); draft.Path != chosen || draft.Name != "inner" || r.App.Sidebar.Picker.Showing() {
		t.Fatalf("the choice did not fill the path: %q named %q", draft.Path, draft.Name)
	}

	// Save is pending while the Node works: disabled, and the draft stays.
	created := count("create_project")
	gate := make(chan struct{})
	hold.Store(&gate)
	all := tester.AccessAll()
	for i := len(all) - 1; i >= 0; i-- {
		if all[i].Role == ui.RoleButton && all[i].Name == zh("settings_save") {
			tester.ClickAt(all[i].Bounds.X+all[i].Bounds.W/2, all[i].Bounds.Y+all[i].Bounds.H/2)
			break
		}
	}
	tester.Frame()
	save, ok := tester.AccessByRole(ui.RoleButton, zh("settings_save"))
	if draft := r.App.Sidebar.Draft(); !ok || !save.Disabled || !draft.Pending || !draft.Open {
		t.Fatalf("Save is not pending while the Node works: %+v", save)
	}
	if _, exists := stored("inner"); exists {
		t.Fatal("the project exists before the Node answered")
	}
	hold.Store(nil)
	close(gate)
	settle()
	settle()
	project, exists := stored("inner")
	if !exists || project.Path != chosen || project.Appearance.Icon != "terminal" || project.Appearance.Color != "blue" || count("create_project") != created+1 {
		t.Fatalf("create_project did not carry the draft: %+v", project)
	}
	row := shown("inner")
	if row == nil || row.Icon != "terminal" || row.Color != "blue" || r.App.Sidebar.Draft().Open || !slices.Contains(tester.Texts(), zh("project_icon_terminal")) {
		t.Fatalf("the sidebar does not show the saved appearance: %+v", row)
	}
	if plain := shown("Plain folder"); plain == nil || plain.Icon != "folder" || plain.Color != "none" {
		t.Fatalf("a project without a choice is not the plain folder: %+v", plain)
	}
	shots("project-sidebar")

	// Editing sends the changed appearance with update_project.
	menu("inner")
	items := tester.Menu()
	for _, key := range []string{"workspace_project", "project_edit", "workspace_new_session", "workspace_copy_path", "project_remove"} {
		if !slices.Contains(items, zh(key)) {
			t.Errorf("the project menu lacks %s: %q", key, items)
		}
	}
	if slices.Contains(items, zh("project_trust")) {
		t.Errorf("the live project menu offers the preview's trust: %q", items)
	}
	if err := tester.ChooseMenuItem(zh("project_edit")); err != nil {
		t.Fatal(err)
	}
	settle()
	if draft := r.App.Sidebar.Draft(); draft.Name != "inner" || draft.Path != chosen || draft.Icon != "terminal" || draft.Color != "blue" || tester.HasText(zh("project_clone")) {
		t.Fatalf("editing did not open on the project: %+v", draft)
	}
	press(zh("project_appearance"))
	press(zh("project_color_red"))
	press(zh("project_icon_science"))
	tester.Key(0, ui.KeyEscape)
	settle()
	press(zh("settings_save"))
	settle()
	project, _ = stored("inner")
	if project.Appearance.Icon != "science" || project.Appearance.Color != "red" || count("update_project") != 1 {
		t.Fatalf("update_project did not carry the appearance: %+v after %d updates", project, count("update_project"))
	}
	if row := shown("inner"); row == nil || row.Icon != "science" || row.Color != "red" {
		t.Fatalf("the sidebar kept the old appearance: %+v", row)
	}

	// A remote owner is browsed through its own controller: a second real
	// Node, bound the way a paired Host is.
	other, err := node.StartWithOptions(r.ctx, filepath.Join(t.TempDir(), "remote"), node.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer other.Stop(context.Background())
	var mu sync.Mutex
	var kinds []string
	remote := client.New(firstRunTransport{Transport: link.NewLocal(other.ID(), other.ID(), other), mu: &mu, kinds: &kinds})
	if err := remote.Start(r.ctx); err != nil {
		t.Fatal(err)
	}
	defer remote.Close()
	host := remote.Target().String()
	if r.bindings == nil {
		r.bindings = map[string]*Runtime{}
	}
	bound := r.newBinding(remote)
	defer bound.Close()
	r.bindings[host] = bound
	defer delete(r.bindings, host)
	settle()
	local := count("browse_files")
	r.App.Sidebar.OpenEditorAtHost(host)
	settle()
	field(zh("project_path"))
	tester.Type(root)
	settle()
	press(zh("directory_title"))
	settle()
	mu.Lock()
	browsed := slices.Contains(kinds, "browse_files")
	mu.Unlock()
	if !browsed || count("browse_files") != local || !tester.HasText("alpha") {
		t.Fatalf("the remote directory was not read through its controller: remote %v: %q", browsed, tester.Texts())
	}
	if err := tester.Click("alpha"); err != nil {
		t.Fatal(err)
	}
	settle()
	press(zh("directory_title"))
	if draft := r.App.Sidebar.Draft(); draft.Path != filepath.Join(root, "alpha") || draft.Name != "alpha" {
		t.Fatalf("the remote choice did not fill the path: %+v", draft)
	}
	// Saving creates the project on that Host, not on the local Node.
	created = count("create_project")
	press(zh("settings_save"))
	settle()
	settle()
	projects := remote.Snapshot().Projects
	if _, here := stored("alpha"); here || count("create_project") != created || len(projects) != 1 || projects[0].Path != filepath.Join(root, "alpha") {
		t.Fatalf("the remote project was not created on its Host: %+v", projects)
	}
	if row := shown("alpha"); row == nil || row.Host != host {
		t.Fatalf("the sidebar does not show the remote project: %+v", row)
	}

	// Removal names the project and what happens, then reports the outcome.
	menu("inner")
	if err := tester.ChooseMenuItem(zh("project_remove")); err != nil {
		t.Fatal(err)
	}
	settle()
	if !tester.HasText(zh("project_remove_named", "name", chosen)) {
		t.Fatalf("the confirmation does not name the project and the consequence: %q", tester.Texts())
	}
	press(zh("project_remove"))
	settle()
	if _, exists := stored("inner"); exists || shown("inner") != nil || count("remove_project") != 1 || !tester.HasText(zh("project_removed")) {
		t.Fatalf("the project was not removed: %q", tester.Texts())
	}
	if _, err := os.Stat(chosen); err != nil {
		t.Fatal("removing the project touched its directory:", err)
	}
}

// firstRunLocation shows the context bar's location with the bundled
// packages mounted: one project control on a chat and on a plain folder's
// draft, and the project with its branch on one line in a Git project. The
// worktrees package's own project control is not drawn beside the native
// one. It is a step of firstRun.
func firstRunLocation(t *testing.T, r *Runtime, tester *ui.Tester, settle func(), shots func(string)) {
	zh := func(key string) string { return i18n.In("zh-CN", key) }
	// controls are the context bar's controls: its small menu buttons.
	controls := func() (bar []ui.AccessInfo) {
		for _, info := range tester.AccessAll() {
			if b := info.Bounds; info.Role == ui.RoleMenuButton && b.H == 24 && b.W > 0 && b.Y > shell.HeaderHeight {
				bar = append(bar, info)
			}
		}
		return
	}
	names := func() (all []string) {
		for _, info := range controls() {
			all = append(all, info.Name)
		}
		return
	}
	choose := func(project string) {
		t.Helper()
		bar := controls()
		if len(bar) == 0 {
			t.Fatalf("no location control: %q", tester.Texts())
		}
		tester.ClickAt(bar[0].Bounds.X+bar[0].Bounds.W/2, bar[0].Bounds.Y+bar[0].Bounds.H/2)
		settle()
		for _, role := range []ui.Role{ui.RoleMenuItem, ui.RoleMenuItemCheckBox, ui.RoleMenuItemRadio} {
			if item, ok := tester.AccessByRole(role, project); ok {
				tester.ClickAt(item.Bounds.X+item.Bounds.W/2, item.Bounds.Y+item.Bounds.H/2)
				settle()
				settle()
				return
			}
		}
		t.Fatalf("the location menu does not offer %q: %q", project, tester.Texts())
	}
	plain, repository := t.TempDir(), t.TempDir()
	if output, err := exec.Command("git", "-C", repository, "init", "-b", "main").CombinedOutput(); err != nil {
		t.Fatalf("git init: %v: %s", err, output)
	}
	for name, path := range map[string]string{"Plain folder": plain, "Git folder": repository} {
		if _, err := r.controller.Do(r.ctx, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
			Name: name, Path: path, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
		}}); err != nil {
			t.Fatal(err)
		}
	}
	r.App.Navigate(shell.ConversationPage)
	r.App.Sidebar.NewChat()
	settle()
	settle()
	// A chat without a project: the one control, and no branch.
	if got := names(); !slices.Equal(got, []string{zh("composer_no_project")}) {
		t.Fatalf("a chat's context bar shows the controls %q, want the one location", got)
	}
	shots("context-new-chat")
	// A project that is not a Git repository has no branch to choose.
	choose("Plain folder")
	if got := names(); !slices.Equal(got, []string{"Plain folder"}) {
		t.Errorf("a plain folder's context bar shows the controls %q, want the project alone", got)
	}
	// A Git project: the project, then its branch, on one line.
	choose("Git folder")
	bar := controls()
	if len(bar) != 2 || bar[0].Name != "Git folder" || bar[1].Name != "main" {
		t.Fatalf("a Git project's context bar shows the controls %q, want the project and its branch", names())
	}
	if bar[0].Bounds.Y != bar[1].Bounds.Y || bar[1].Bounds.X < bar[0].Bounds.X+bar[0].Bounds.W {
		t.Errorf("the project %v and the branch %v are not side by side on one line", bar[0].Bounds, bar[1].Bounds)
	}
	shots("context-git-project")
}

// firstRunConnectors shows the connectors of the new profile and of a server
// added by hand, served in this process: each row's own status, the menu of
// the actions it has, and what each item does. It is a step of
// firstRunChinese and reaches no outside endpoint.
func firstRunConnectors(t *testing.T, r *Runtime, tester *ui.Tester, settle func(), sent func() []string, shots func(string)) {
	t.Setenv("SAILRY_HELPERS_DIR", t.TempDir()) // no computer helper, as a run from source
	zh := func(key string, a ...any) string { return i18n.In("zh-CN", key, a...) }
	count := func(kind string) (n int) {
		for _, name := range sent() {
			if name == kind {
				n++
			}
		}
		return n
	}
	service := func(name string) *settings.ConnStatus {
		t.Helper()
		for i := range r.App.Settings.Services {
			if s := &r.App.Settings.Services[i]; s.Name == name {
				status := settings.StatusOf(s)
				return &status
			}
		}
		t.Fatalf("no %q connector: %+v", name, r.App.Settings.Services)
		return nil
	}
	// open opens the menu of the row named name and returns its items.
	open := func(name string) (items []ui.AccessInfo) {
		t.Helper()
		row := -1
		for i := range r.App.Settings.Services {
			if r.App.Settings.Services[i].Name == name {
				row = i
			}
		}
		var menus []ui.AccessInfo
		for _, info := range tester.AccessAll() {
			if info.Name == zh("more") {
				menus = append(menus, info)
			}
		}
		if row < 0 || len(menus) != len(r.App.Settings.Services) {
			t.Fatalf("row %q is %d of %d menus", name, row, len(menus))
		}
		if menus[row].Disabled {
			return nil
		}
		tester.ClickAt(menus[row].Bounds.X+menus[row].Bounds.W/2, menus[row].Bounds.Y+menus[row].Bounds.H/2)
		settle()
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleMenuItem {
				items = append(items, info)
			}
		}
		return items
	}
	// choose opens the row's menu, which must list want alone, and picks item.
	choose := func(name string, want []string, item string) {
		t.Helper()
		items := open(name)
		var listed []string
		var target *ui.AccessInfo
		for i := range items {
			listed = append(listed, items[i].Name)
			if items[i].Name == zh(item) {
				target = &items[i]
			}
		}
		var labels []string
		for _, key := range want {
			labels = append(labels, zh(key))
		}
		if !slices.Equal(listed, labels) || target == nil {
			t.Fatalf("the menu of %q lists %q, want %q with %q", name, listed, labels, zh(item))
		}
		tester.ClickAt(target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2)
		settle()
	}
	press := func(name string) {
		t.Helper()
		// The last one: a dialog's own button comes after its frame's.
		var info *ui.AccessInfo
		for _, x := range tester.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == name {
				info = &x
			}
		}
		if info == nil {
			t.Fatalf("no %q button: %q", name, tester.Texts())
		}
		tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		settle()
	}
	// field is the text of the last text field named name.
	field := func(name string) (value string) {
		for _, x := range tester.AccessAll() {
			if x.Role == ui.RoleTextField && x.Name == name {
				value = x.Value
			}
		}
		return value
	}
	dismiss := func() {
		for range 8 {
			info, ok := tester.AccessByRole(ui.RoleButton, zh("close"))
			if !ok {
				return
			}
			tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
			settle()
		}
	}

	r.App.Settings.Open(settings.Connectors)
	settle()
	settle()
	// Nothing was asked to connect: no row has failed. The disabled package
	// says so, the server that takes an optional key and the one that needs
	// an account are not connected.
	if tester.HasText(zh("connector_mcp_failed")) || count("check_plugin_mcp") != 0 {
		t.Errorf("a new profile shows a failed connector or checked one: %q", tester.Texts())
	}
	if st := service("computer"); st.Problem != "connector_package_disabled" {
		t.Errorf("computer %+v", *st)
	}
	if st := service("context7"); st.Problem != "connector_not_connected" || st.Failed {
		t.Errorf("context7 %+v", *st)
	}
	if st := service("github"); st.Problem != "connector_not_connected" || st.Failed {
		t.Errorf("github %+v", *st)
	}
	shots("connectors-fresh")
	if items := open("context7"); len(items) == 0 {
		t.Fatal("context7 has no menu")
	}
	shots("connectors-fresh-menu")
	tester.Key(0, ui.KeyEscape)
	settle()

	// Each item does its action; none of them here reaches the network.
	before := len(sent())
	choose("computer", []string{"connector_details"}, "connector_details")
	if !tester.HasText(zh("connector_mcp_server")) || !tester.HasText("desktop") {
		t.Errorf("computer's details: %q", tester.Texts())
	}
	press(zh("close"))
	choose("context7", []string{"connector_add_credentials", "connector_mcp_reconnect", "connector_details"}, "connector_add_credentials")
	if _, ok := tester.AccessByRole(ui.RoleButton, zh("settings_cancel")); !ok || !tester.HasText("Context7") {
		t.Fatalf("Add credentials did not open context7's editor: %q", tester.Texts())
	}
	shots("connectors-add-credentials")
	press(zh("settings_cancel"))
	choose("github", []string{"connector_connect", "connector_details"}, "connector_connect")
	if _, ok := tester.AccessByRole(ui.RoleButton, zh("settings_cancel")); !ok || !tester.HasText("GitHub") {
		t.Fatalf("Connect did not open github's editor: %q", tester.Texts())
	}
	press(zh("settings_cancel"))
	if acted := sent()[before:]; len(acted) != 0 {
		t.Errorf("opening details and editors sent %q", acted)
	}
	// The details of a plugin's own server lead to its editor, on the
	// configuration read from the Node.
	choose("context7", []string{"connector_add_credentials", "connector_mcp_reconnect", "connector_details"}, "connector_details")
	press(zh("connector_configure"))
	settle()
	if _, ok := tester.AccessByRole(ui.RoleButton, zh("settings_save")); !ok || count("read_plugin_mcp") != 1 || !strings.HasPrefix(field(zh("mcp_url")), "https://") || tester.HasText(zh("connector_mcp_server")) {
		t.Fatalf("Configure in context7's details did not open its configuration: %d reads, address %q: %q", count("read_plugin_mcp"), field(zh("mcp_url")), tester.Texts())
	}
	shots("connectors-package-configure")
	press(zh("settings_cancel"))

	// The enabled computer package without its helper is unavailable and
	// says why; it has nothing to reconnect.
	computer, _ := r.connectorPackage("mcp:computer")
	r.connectorCommand(protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{Name: "computer", ExpectedRevision: computer.Summary.Revision, Enabled: true}}, nil)
	settle()
	settle()
	if st := service("computer"); st.Problem != "connector_unavailable" || !strings.Contains(st.Reason, "helper") {
		t.Errorf("computer without its helper %+v", *st)
	}
	choose("computer", []string{"connector_details"}, "connector_details")
	if !tester.HasText(service("computer").Reason) {
		t.Errorf("computer's details do not give the reason: %q", tester.Texts())
	}
	shots("connectors-helper-missing-details")
	press(zh("close"))

	// A server added by hand connects, lists its tools and reconnects.
	fixture := sdk.NewServer(&sdk.Implementation{Name: "fixture", Version: "1"}, nil)
	for _, name := range []string{"echo", "ping"} {
		sdk.AddTool(fixture, &sdk.Tool{Name: name}, func(context.Context, *sdk.CallToolRequest, struct{}) (*sdk.CallToolResult, any, error) {
			return &sdk.CallToolResult{}, nil, nil
		})
	}
	// keys are the credentials new sessions were opened with since called
	// last read them; closing a kept session still carries its old one.
	var keysMu sync.Mutex
	var keys []string
	called := func() []string {
		keysMu.Lock()
		defer keysMu.Unlock()
		seen := slices.Compact(keys)
		keys = nil
		return seen
	}
	handler := sdk.NewStreamableHTTPHandler(func(*http.Request) *sdk.Server { return fixture }, nil)
	endpoint := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, request *http.Request) {
		if request.Header.Get("Mcp-Session-Id") == "" {
			keysMu.Lock()
			keys = append(keys, request.Header.Get("X-Key"))
			keysMu.Unlock()
		}
		handler.ServeHTTP(w, request)
	}))
	defer func() {
		endpoint.CloseClientConnections()
		endpoint.Close()
	}()
	saved := false
	r.App.Settings.ConnectorActions.SaveMCP(settings.MCPDraft{Name: "fixture", Server: viewmodel.ConnectorServer{Name: "fixture", Transport: 1, URL: endpoint.URL, Env: []viewmodel.EnvVar{{Key: "X-Key", Value: "first"}}}}, func(ok bool) { saved = ok })
	settle()
	settle()
	if !saved || count("install_mcp") != 1 || count("check_plugin_mcp") != 1 {
		t.Fatalf("adding the server: saved %v, %d installs, %d checks", saved, count("install_mcp"), count("check_plugin_mcp"))
	}
	connected := zh("connector_mcp_reconnected", "n", 2)
	if st := service("fixture"); st.Tools != 2 || !tester.HasText(connected) {
		t.Fatalf("the added server %+v: %q", *st, tester.Texts())
	}
	if seen := called(); !slices.Equal(seen, []string{"first"}) {
		t.Errorf("the added server was called with %q", seen)
	}
	dismiss()
	shots("connectors-connected")
	manual := []string{"connector_configure", "connector_remove", "connector_mcp_reconnect", "connector_details"}
	if items := open("fixture"); len(items) != len(manual) {
		t.Fatalf("the connected server's menu has %d items", len(items))
	}
	shots("connectors-connected-menu")
	tester.Key(0, ui.KeyEscape)
	settle()
	choose("fixture", manual, "connector_mcp_reconnect")
	settle()
	if count("check_plugin_mcp") != 2 || !tester.HasText(connected) || service("fixture").Tools != 2 {
		t.Errorf("Reconnect: %d checks, %+v: %q", count("check_plugin_mcp"), *service("fixture"), tester.Texts())
	}
	dismiss()

	// Configure reads the saved server into its editor: the definition and
	// the slot that holds a credential, never the credential. Saving it
	// untouched keeps the credential; Clear saves the slot empty.
	choose("fixture", manual, "connector_configure")
	settle()
	if !tester.HasText(zh("mcp_secret_saved")) || tester.HasText("first") || count("read_plugin_settings") != 1 {
		t.Fatalf("Configure did not load the saved server: %d reads, %q", count("read_plugin_settings"), tester.Texts())
	}
	if address, slot, value := field(zh("mcp_url")), field(zh("mcp_slot_name")), field(zh("mcp_slot_value")); address != endpoint.URL || slot != "X-Key" || value != "" {
		t.Errorf("the editor shows %q with %q as %q", address, slot, value)
	}
	shots("connectors-configure")
	called()
	press(zh("settings_save"))
	settle()
	if seen := called(); count("install_mcp") != 2 || count("check_plugin_mcp") != 3 || !slices.Equal(seen, []string{"first"}) || service("fixture").Tools != 2 {
		t.Fatalf("saving the untouched server: %d installs, %d checks, called with %q, %+v: %q", count("install_mcp"), count("check_plugin_mcp"), seen, *service("fixture"), tester.Texts())
	}
	dismiss()
	choose("fixture", manual, "connector_configure")
	settle()
	if err := tester.Click(zh("mcp_clear_secret")); err != nil {
		t.Fatal(err)
	}
	settle()
	if !tester.HasText(zh("mcp_secret_cleared")) {
		t.Errorf("Clear did not mark the slot: %q", tester.Texts())
	}
	press(zh("settings_save"))
	settle()
	if seen := called(); count("install_mcp") != 3 || count("check_plugin_mcp") != 4 || !slices.Equal(seen, []string{""}) {
		t.Fatalf("saving the cleared slot: %d installs, %d checks, called with %q: %q", count("install_mcp"), count("check_plugin_mcp"), seen, tester.Texts())
	}
	dismiss()
	choose("fixture", manual, "connector_configure")
	settle()
	if tester.HasText(zh("mcp_secret_saved")) {
		t.Errorf("the cleared slot still holds a credential: %q", tester.Texts())
	}
	press(zh("settings_cancel"))

	// With the endpoint gone the kept connection cannot be closed for sure:
	// that Reconnect is uncertain, said as such, and the row has failed. The
	// next one fails outright with its reason: in the toast, on the row and
	// in the details.
	endpoint.CloseClientConnections() // a kept stream would hold Close
	endpoint.Close()
	choose("fixture", manual, "connector_mcp_reconnect")
	settle()
	if st := service("fixture"); count("check_plugin_mcp") != 5 || !st.Failed || !tester.HasText(zh("connector_unknown")) {
		t.Fatalf("Reconnect over a lost connection: %d checks, %+v: %q", count("check_plugin_mcp"), *st, tester.Texts())
	}
	dismiss()
	choose("fixture", manual, "connector_mcp_reconnect")
	settle()
	failed := service("fixture")
	if count("check_plugin_mcp") != 6 || !failed.Failed || failed.Reason == "" {
		t.Fatalf("Reconnect to a closed endpoint: %d checks, %+v", count("check_plugin_mcp"), *failed)
	}
	if tester.HasText(zh("connector_unknown")) || !tester.HasText(failed.Reason) {
		t.Errorf("the failure's toast does not give %q: %q", failed.Reason, tester.Texts())
	}
	shots("connectors-failed")
	dismiss()
	choose("fixture", manual, "connector_details")
	if !tester.HasText(zh("connector_mcp_error")) || !tester.HasText(failed.Reason) {
		t.Errorf("the failed server's details: %q", tester.Texts())
	}
	shots("connectors-failed-details")
	press(zh("close"))

	// Configure opens the server's editor; the switch stops the server, which
	// then has nothing to reconnect; Delete removes it after its prompt.
	choose("fixture", manual, "connector_configure")
	if _, ok := tester.AccessByRole(ui.RoleButton, zh("settings_save")); !ok {
		t.Fatalf("Configure did not open the server's editor: %q", tester.Texts())
	}
	press(zh("settings_cancel"))
	if err := tester.Click(zh("connector_server_allowed") + " fixture"); err != nil {
		t.Fatal(err)
	}
	settle()
	settle()
	if st := service("fixture"); count("set_plugin_mcp_allowed") != 1 || st.Problem != "connector_server_disabled" {
		t.Fatalf("the switch: %d commands, %+v", count("set_plugin_mcp_allowed"), *st)
	}
	dismiss()
	stopped := []string{"connector_configure", "connector_remove", "connector_details"}
	choose("fixture", stopped, "connector_remove")
	press(zh("connector_remove"))
	settle()
	if count("remove_plugin") != 1 || slices.ContainsFunc(r.App.Settings.Services, func(s viewmodel.ConnectorService) bool { return s.Name == "fixture" }) {
		t.Errorf("Delete: %d removals, %+v", count("remove_plugin"), r.App.Settings.Services)
	}
	dismiss()
}
