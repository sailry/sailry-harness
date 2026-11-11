package shell

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// recorded is a composer's owner that writes down what it is asked.
type recorded struct {
	composer.Actions
	log []string
}

func (r *recorded) note(format string, args ...any) {
	r.log = append(r.log, fmt.Sprintf(format, args...))
}
func (r *recorded) Send(m composer.Message)       { r.note("send %s", m.Text) }
func (r *recorded) Upload(a *composer.Attachment) { r.note("upload %s", a.Name) }
func (r *recorded) Discard(*composer.Attachment)  {}
func (r *recorded) RemoveQueued(id string)        { r.note("remove %s", id) }
func (r *recorded) WithdrawGuidance(id string)    { r.note("withdraw %s", id) }
func (r *recorded) SendQueued(id string)          { r.note("send-queued %s", id) }
func (r *recorded) Compact()                      { r.note("compact") }
func (r *recorded) Stop()                         { r.note("stop") }

// composerWith builds the selected preview conversation's composer alone,
// zh-CN and dark for the saved frames, with a recording owner.
func composerWith(t *testing.T, width int, prepare func(*composer.State, *composer.Context)) (*ui.Tester, *composer.State, *recorded) {
	t.Helper()
	a := New(preview.Load())
	cv := a.Data.Find(a.Sidebar.Selected)
	comp := a.Composer(cv.ID)
	actions := &recorded{}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		kit.SetPreferences(c, kit.Preferences{})
		ctx := preview.Composer(a.Data, cv, comp.Worktree)
		ctx.Interactions, ctx.Queue = composer.Interactions{}, nil
		ctx.Activity = composer.Activity{Connected: true}
		if prepare != nil {
			prepare(comp, ctx)
		}
		ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, comp, ctx, actions) })
	}, width, 520, opaque)
	tt.SetLocale("zh-CN")
	tt.SetDark(true)
	tt.Frame()
	tt.Frame()
	return tt, comp, actions
}

func zh(key string, args ...any) string { return i18n.In("zh-CN", key, args...) }

// button is the enabled or disabled button named name, clicked at its centre.
func pressButton(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	info, ok := control(tt, name)
	if !ok {
		t.Fatalf("no control %q: %q", name, tt.Texts())
	}
	b := info.Bounds
	tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
	tt.Frame()
	tt.Frame()
}

// control is the first control named name: a button or a menu button.
func control(tt *ui.Tester, name string) (ui.AccessInfo, bool) {
	for _, e := range tt.AccessAll() {
		if e.Name == name && e.Role != ui.RoleText && e.Role != ui.RoleNone {
			return e, true
		}
	}
	return ui.AccessInfo{}, false
}

// composerParity are the composer's controls that follow the reference: the
// packages' modes, the host control, a child session's tray, the queue row
// states, pasted and dropped files, /compact's focus and a stale reference.
func composerParity(t *testing.T) {
	t.Run("plugin modes", func(t *testing.T) {
		var chosen []string
		selected := ""
		modes := func(s *composer.State, ctx *composer.Context) {
			ctx.PluginModes = []composer.PluginMode{
				{Key: "goals/goal", Label: "目标模式", Description: "按目标持续推进", Icon: icons.Tasks, Selected: selected == "goals/goal"},
				{Key: "review/audit", Label: "审查模式", Description: "只读审查", Disabled: true},
			}
			ctx.SelectMode = func(key string) {
				chosen = append(chosen, key)
				selected = key
				if !strings.Contains(key, "/") {
					selected = ""
				}
			}
		}
		tt, comp, _ := composerWith(t, 960, modes)
		pressButton(t, tt, zh("composer_mode_code"))
		if !tt.HasText("目标模式") || !tt.HasText("审查模式") || !tt.HasText("按目标持续推进") {
			t.Fatalf("the mode menu lists no package mode: %q", tt.Texts())
		}
		settled(t, tt, "parity-mode-menu")
		click(t, tt, "目标模式")
		tt.Frame()
		tt.Frame()
		if !slices.Equal(chosen, []string{"goals/goal"}) {
			t.Fatalf("choosing the package mode selected %v", chosen)
		}
		if _, ok := control(tt, "目标模式"); !ok {
			t.Errorf("the mode control does not name the package mode: %q", tt.Texts())
		}
		// Shift+Tab goes on from the package's mode to the first built-in
		// one, then through them back to it, past the disabled mode.
		click(t, tt, zh("composer"))
		chosen = nil
		for range 4 {
			tt.Key(ui.Shift, ui.KeyTab)
			tt.Frame()
		}
		if want := []string{"code", "plan", "flagship", "goals/goal"}; !slices.Equal(chosen, want) {
			t.Errorf("Shift+Tab cycled %v, want %v", chosen, want)
		}
		if comp.Mode != composer.Flagship {
			t.Errorf("the built-in mode under the cycle is %v", comp.Mode)
		}
		// The compact settings popover lists them on its mode page.
		selected = ""
		tt, _, _ = composerWith(t, 440, modes)
		settings := zh("composer_settings")
		var found bool
		for _, e := range tt.AccessAll() {
			if e.Role == ui.RoleButton && strings.HasPrefix(e.Name, settings) {
				tt.ClickAt(e.Bounds.X+e.Bounds.W/2, e.Bounds.Y+e.Bounds.H/2)
				found = true
				break
			}
		}
		if !found {
			t.Fatalf("no settings button at the compact width: %q", tt.Texts())
		}
		tt.Frame()
		tt.Frame()
		click(t, tt, zh("composer_mode_code"))
		tt.Frame()
		tt.Frame()
		if !tt.HasText("目标模式") || !tt.HasText("审查模式") {
			t.Fatalf("the compact mode page lists no package mode: %q", tt.Texts())
		}
		settled(t, tt, "parity-mode-compact")
		chosen = nil
		click(t, tt, "目标模式")
		tt.Frame()
		if !slices.Equal(chosen, []string{"goals/goal"}) {
			t.Errorf("the compact page selected %v", chosen)
		}
	})

	t.Run("host control", func(t *testing.T) {
		draft := func(s *composer.State, ctx *composer.Context) { ctx.LocationLocked = false }
		tt, comp, _ := composerWith(t, 960, draft)
		local, remote := zh("local_host_name"), zh("remote_host_name")
		if _, ok := control(tt, local); !ok {
			t.Fatalf("no host control with two hosts: %q", tt.Texts())
		}
		settled(t, tt, "parity-host-control")
		comp.Project, comp.Model = "sailry-go", "preview/model"
		pressButton(t, tt, local)
		settled(t, tt, "parity-host-menu")
		click(t, tt, remote)
		tt.Frame()
		tt.Frame()
		if comp.Host != "remote" || comp.Project != "" || comp.Model != "" {
			t.Fatalf("choosing the remote host left host %q, project %q, model %q", comp.Host, comp.Project, comp.Model)
		}
		if _, ok := control(tt, remote); !ok {
			t.Errorf("the control does not name the chosen host: %q", tt.Texts())
		}
		// A session stays on its host: the control names it and is disabled.
		tt, _, _ = composerWith(t, 960, func(s *composer.State, ctx *composer.Context) { ctx.LocationLocked = true })
		if b, ok := control(tt, local); !ok || !b.Disabled {
			t.Errorf("a session's host control: %+v, found %v", b, ok)
		}
		// One host offers no choice: no control.
		tt, _, _ = composerWith(t, 960, func(s *composer.State, ctx *composer.Context) { ctx.Hosts = ctx.Hosts[:1] })
		if _, ok := control(tt, local); ok {
			t.Error("a host control shows with one host")
		}
	})

	queue := []composer.Queued{
		{ID: "plain", Text: "补充单元测试"},
		{ID: "guided", Text: "先看失败的用例", Guided: true},
		{ID: "joining", Text: "保留现有接口", Guided: true, Joining: true},
		{ID: "notice", Text: "后台命令已结束", Notice: true},
	}
	load := func(text string) func(func(string, bool)) {
		return func(done func(string, bool)) { done(text, true) }
	}
	for i := range queue {
		queue[i].Load = load(queue[i].Text)
	}
	states := func(tt *ui.Tester, key string) (all, off int) { return queueButtons(tt, zh(key)) }

	t.Run("queue row states", func(t *testing.T) {
		tt, _, actions := composerWith(t, 960, func(s *composer.State, ctx *composer.Context) {
			ctx.Queue = slices.Clone(queue)
			ctx.Outgoing = &composer.Queued{Text: "正在发送的消息", Guided: true}
		})
		settled(t, tt, "parity-queue-states")
		// Edit: the plain row's alone acts; both guided rows' and the
		// outgoing row's are disabled; a notice has none.
		if all, off := states(tt, "queue_edit"); all != 4 || off != 3 {
			t.Errorf("Edit: %d buttons, %d disabled", all, off)
		}
		// Send now: off on guidance aimed at another turn and on the
		// outgoing row, on for the row the running turn is taking.
		// (The composer's own Send, disabled on an empty draft, has the name.)
		if all, off := states(tt, "queue_send_now"); all != 5 || off != 3 {
			t.Errorf("Send now: %d buttons, %d disabled", all, off)
		}
		if all, off := states(tt, "queue_delete"); all != 5 || off != 3 {
			t.Errorf("Remove: %d buttons, %d disabled", all, off)
		}
		if all, off := states(tt, "queue_withdraw"); all != 2 || off != 0 {
			t.Errorf("Withdraw: %d buttons, %d disabled", all, off)
		}
		if !tt.HasText("正在发送的消息") {
			t.Errorf("no outgoing row: %q", tt.Texts())
		}
		pressButton(t, tt, zh("queue_withdraw"))
		if !slices.Equal(actions.log, []string{"withdraw guided"}) {
			t.Errorf("Withdraw sent %v", actions.log)
		}
	})

	t.Run("outgoing row", func(t *testing.T) {
		tt, _, _ := composerWith(t, 960, func(s *composer.State, ctx *composer.Context) {
			ctx.Queue = slices.Clone(queue[:2])
			ctx.Outgoing = &composer.Queued{Text: "正在发送的消息", Guided: true}
		})
		row, ok := tt.Find("正在发送的消息")
		if !ok || row.H == 0 {
			t.Fatalf("the outgoing row is not shown: %q", tt.Texts())
		}
		settled(t, tt, "parity-queue-outgoing")
	})

	t.Run("child session", func(t *testing.T) {
		connected := true
		tt, _, actions := composerWith(t, 960, func(s *composer.State, ctx *composer.Context) {
			ctx.Child, ctx.Readonly = true, true
			ctx.Activity.Connected = connected
			ctx.Queue = slices.Clone(queue[:2])
		})
		settled(t, tt, "parity-child-session")
		if _, ok := tt.Access(zh("composer")); ok {
			t.Error("a child session shows the composer's input")
		}
		if all, _ := states(tt, "queue_send_now"); all != 0 {
			t.Errorf("a child session's tray has %d Send now", all)
		}
		if all, _ := states(tt, "queue_edit"); all != 0 {
			t.Errorf("a child session's tray has %d Edit", all)
		}
		if all, off := states(tt, "queue_read"); all != 2 || off != 1 {
			t.Errorf("Read: %d buttons, %d disabled", all, off)
		}
		pressButton(t, tt, zh("queue_read"))
		settled(t, tt, "parity-child-read")
		for _, key := range []string{"message_copy", "close", "queue_delete"} {
			if _, ok := tt.AccessByRole(ui.RoleButton, zh(key)); !ok {
				t.Fatalf("the read-only editor has no %s: %q", key, tt.Texts())
			}
		}
		pressButton(t, tt, zh("message_copy"))
		if tt.Clipboard() != "补充单元测试" {
			t.Errorf("Copy wrote %q", tt.Clipboard())
		}
		pressButton(t, tt, zh("close"))
		if all, _ := states(tt, "queue_read"); all != 2 {
			t.Errorf("Close left %d rows", all)
		}
		pressButton(t, tt, zh("queue_read"))
		// The editor's Remove is the last of that name.
		var remove ui.AccessInfo
		for _, e := range tt.AccessAll() {
			if e.Role == ui.RoleButton && e.Name == zh("queue_delete") && e.Bounds.W > 24 {
				remove = e
			}
		}
		if remove.Name == "" {
			t.Fatal("no Remove in the read-only editor")
		}
		tt.ClickAt(remove.Bounds.X+remove.Bounds.W/2, remove.Bounds.Y+remove.Bounds.H/2)
		tt.Frame()
		if !slices.Equal(actions.log, []string{"remove plain"}) {
			t.Errorf("Remove sent %v", actions.log)
		}
		// Away from its Host the tray's actions are held.
		connected = false
		tt.Frame()
		tt.Frame()
		if all, off := states(tt, "queue_delete"); all == 0 || off != all {
			t.Errorf("disconnected: %d Remove, %d disabled", all, off)
		}
	})

	t.Run("paste and drop", func(t *testing.T) {
		dir := t.TempDir()
		var paths []string
		for i := range 9 {
			path := filepath.Join(dir, fmt.Sprintf("note-%d.txt", i))
			if err := os.WriteFile(path, []byte("x"), 0o600); err != nil {
				t.Fatal(err)
			}
			paths = append(paths, path)
		}
		folder := filepath.Join(dir, "assets")
		if err := os.Mkdir(folder, 0o700); err != nil {
			t.Fatal(err)
		}
		tt, comp, actions := composerWith(t, 960, nil)
		click(t, tt, zh("composer"))
		files, err := transfer.FileData(paths[0], paths[1])
		if err != nil {
			t.Fatal(err)
		}
		tt.SetClipboardData(files)
		tt.Key(ui.Cmd, ui.KeyV)
		tt.Frame()
		if len(comp.Attachments) != 2 || comp.Attachments[0].Path != paths[0] || comp.Attachments[1].Name != "note-1.txt" {
			t.Fatalf("pasted files: %+v, draft %q", comp.Attachments, comp.Draft)
		}
		if comp.Draft != "" {
			t.Errorf("pasted files left their names in the draft: %q", comp.Draft)
		}
		// A copied picture that is not a PNG, named by its place.
		tt.SetClipboardData(transfer.New(transfer.NewItem(transfer.Bytes("image/jpeg", []byte{0xff, 0xd8, 0xff, 0xd9}))))
		tt.Key(ui.Cmd, ui.KeyV)
		tt.Frame()
		if want := zh("chat_clipboard_image", "count", 3) + ".jpg"; len(comp.Attachments) != 3 || comp.Attachments[2].Name != want || comp.Attachments[2].Mime != "image/jpeg" {
			t.Fatalf("pasted JPEG: %d attachments, last %+v, want %q", len(comp.Attachments), comp.Attachments[len(comp.Attachments)-1], want)
		}
		// Text stays the draft's.
		tt.SetClipboard("just text")
		tt.Key(ui.Cmd, ui.KeyV)
		tt.Frame()
		if comp.Draft != "just text" || len(comp.Attachments) != 3 {
			t.Errorf("pasted text: draft %q, %d attachments", comp.Draft, len(comp.Attachments))
		}
		// A file and a folder dropped on the surface.
		area, _ := tt.Access(zh("composer"))
		if !tt.DropFiles(area.Bounds.X+area.Bounds.W/2, area.Bounds.Y+area.Bounds.H/2, paths[2], folder) {
			t.Fatal("the composer takes no dropped files")
		}
		tt.Frame()
		if len(comp.Attachments) != 5 || comp.Attachments[4].Path != folder {
			t.Fatalf("dropped file and folder: %+v", comp.Attachments)
		}
		// A batch past the limit is refused whole, with the limit's toast.
		many, err := transfer.FileData(paths[3:8]...)
		if err != nil {
			t.Fatal(err)
		}
		tt.SetClipboardData(many)
		tt.Key(ui.Cmd, ui.KeyV)
		tt.Frame()
		tt.Frame()
		if len(comp.Attachments) != 5 {
			t.Errorf("a batch past the limit attached: %d", len(comp.Attachments))
		}
		if !tt.HasText(zh("chat_attachment_limit")) {
			t.Errorf("no limit toast: %q", tt.Texts())
		}
		uploads := 0
		for _, line := range actions.log {
			if strings.HasPrefix(line, "upload ") {
				uploads++
			}
		}
		if uploads != 5 {
			t.Errorf("%d uploads: %v", uploads, actions.log)
		}
	})

	t.Run("compact focus and stale reference", func(t *testing.T) {
		var focus []string
		stale := false
		tt, comp, actions := composerWith(t, 960, func(s *composer.State, ctx *composer.Context) {
			ctx.Compact = func(text string) { focus = append(focus, text) }
			ctx.Stale = func(r composer.Reference) bool { return stale && r.Kind == composer.Prompt }
		})
		click(t, tt, zh("composer"))
		tt.Type("/compact 保留接口约定")
		tt.Frame()
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		if !slices.Equal(focus, []string{"保留接口约定"}) || comp.Draft != "" || len(actions.log) != 0 {
			t.Fatalf("/compact with a focus: %v, draft %q, sent %v", focus, comp.Draft, actions.log)
		}
		// Chosen from the picker with nothing after it: no focus.
		tt.Type("/compact")
		tt.Frame()
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		if !slices.Equal(focus, []string{"保留接口约定", ""}) || comp.Draft != "" {
			t.Fatalf("/compact from the picker: %q, draft %q", focus, comp.Draft)
		}
		// A command gone since the picker listed it says so and stays out
		// of the draft.
		stale = true
		tt.Type("/review")
		tt.Frame()
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		tt.Frame()
		if !tt.HasText(zh("reference_stale")) {
			t.Errorf("no stale toast: %q", tt.Texts())
		}
		if comp.Draft != "/review" {
			t.Errorf("a stale command changed the draft: %q", comp.Draft)
		}
	})
}

// headerParity are the conversation page's header actions and its own keys:
// the Search button, the session overview, ⌘. to stop, ⌘L to focus the
// composer and ⇧⌘M for the model controls.
func headerParity(t *testing.T) {
	a, tt := newTester(t)
	tt.SetLocale("zh-CN")
	tt.SetDark(true)
	a.Sidebar.Selected = preview.Big
	tt.Frame()
	tt.Frame()
	cv := a.Data.Find(preview.Big)
	if !cv.Running() {
		t.Fatal("the big conversation does not run")
	}
	// The overview: the turn's subagents, its progress, outputs and sources.
	pressButton(t, tt, zh("chat_assets_title"))
	for _, key := range []string{"chat_assets_activity", "chat_assets_progress", "chat_assets_output", "chat_assets_sources"} {
		if !tt.HasText(zh(key)) {
			t.Fatalf("the overview has no %s: %q", key, tt.Texts())
		}
	}
	tt.Advance(300 * time.Millisecond)
	tt.Frame()
	if !a.assets.Open() {
		t.Fatal("the overview is not open")
	}
	shot(t, tt, "parity-overview")
	overview := preview.Overview(cv)
	if len(overview.Children) == 0 || len(overview.Steps) == 0 {
		t.Fatalf("the preview's overview is empty: %+v", overview)
	}
	child := overview.Children[0]
	var row ui.AccessInfo
	for _, e := range tt.AccessAll() {
		if e.Role == ui.RoleButton && strings.HasPrefix(e.Name, child.Name+": ") {
			row = e
		}
	}
	if row.Name == "" {
		t.Fatalf("no row for subagent %q", child.Name)
	}
	tt.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
	tt.Frame()
	tt.Frame()
	tt.Advance(300 * time.Millisecond)
	tt.Frame()
	if !a.PanelOpen || a.children[child.ID] == nil {
		t.Errorf("a subagent's row did not open its session: panel %v", a.PanelOpen)
	}
	if a.assets.Open() {
		t.Error("the overview stays open after a row was chosen")
	}
	// Search opens the conversation's find bar.
	if a.Messages.SearchOpen() {
		t.Fatal("the find bar is open before Search")
	}
	pressButton(t, tt, zh("chat_search"))
	if !a.Messages.SearchOpen() {
		t.Error("Search did not open the find bar")
	}
	settled(t, tt, "parity-header-search")
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	// The conversation's keys.
	if tt.Focused(zh("composer")) {
		t.Fatal("the composer has the focus before ⌘L")
	}
	tt.Key(ui.Cmd, ui.KeyL)
	tt.Frame()
	tt.Frame()
	if !tt.Focused(zh("composer")) {
		t.Error("⌘L did not focus the composer")
	}
	tt.Key(ui.Cmd|ui.Shift, ui.KeyM)
	tt.Frame()
	tt.Frame()
	if !tt.HasText(zh("composer_select_model")) && !tt.HasText(zh("composer_execution_model")) {
		t.Errorf("⇧⌘M did not open the model controls: %q", tt.Texts())
	}
	settled(t, tt, "parity-model-shortcut")
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyPeriod)
	tt.Frame()
	if cv.Running() {
		t.Error("⌘. did not stop the running turn")
	}
	// Their keys are the app's: no shortcut may be recorded on one.
	a.Settings.Keys.Record("app.search")
	for _, chord := range []settings.Chord{{Mods: ui.Cmd, Key: ui.KeyPeriod}, {Mods: ui.Cmd, Key: ui.KeyL}, {Mods: ui.Cmd | ui.Shift, Key: ui.KeyM}} {
		if problem := a.Settings.Keys.Capture(chord); problem != "shortcut_conflict" {
			t.Errorf("recording %v on another shortcut: %q", chord, problem)
		}
	}
	a.Settings.Keys.Cancel()
	// They are the conversation page's: nothing from Settings.
	a.Navigate(SettingsPage)
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyL)
	tt.Frame()
	if a.keyed != "" || a.Page != SettingsPage {
		t.Errorf("⌘L acted outside the conversation page: %q", a.keyed)
	}
}

// settled saves the frame once the layers' motion has ended.
func settled(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	tt.Advance(300 * time.Millisecond)
	tt.Frame()
	shot(t, tt, name)
}
