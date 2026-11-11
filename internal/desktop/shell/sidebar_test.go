package shell

import (
	"cmp"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/picker"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// TestSidebarSessions groups the cases of one behavior; each keeps its own fixture.
func TestSidebarSessions(t *testing.T) {
	t.Run("SidebarOpensConversation", testSidebarOpensConversation)
	t.Run("SidebarShowMore", testSidebarShowMore)
	t.Run("SidebarMenuPinsAndRenames", testSidebarMenuPinsAndRenames)
	t.Run("SidebarRenameKeepsEmptyTitleOut", testSidebarRenameKeepsEmptyTitleOut)
	t.Run("SidebarReordersPins", testSidebarReordersPins)
	t.Run("SidebarDeleteConfirms", testSidebarDeleteConfirms)
	t.Run("SidebarNewSession", testSidebarNewSession)
	t.Run("SidebarGroups", testSidebarGroups)
}

// groupsData is a live-shaped sidebar that tells the three orders apart, for
// the projects, the chats and a project's sessions: projects come oldest
// first and sessions newest first, as a Node lists them, and their names
// and last runs disagree with that. "Keel survey" is pinned.
func groupsData() *viewmodel.Data {
	ago := func(d time.Duration) time.Time { return preview.Now.Add(-d) }
	session := func(id, title string, d time.Duration) *viewmodel.Conversation {
		return &viewmodel.Conversation{ID: id, Title: title, Updated: ago(d)}
	}
	keel := session("keel", "Keel survey", 6*time.Hour)
	keel.Pinned = true
	return &viewmodel.Data{
		Unassigned: []*viewmodel.Conversation{
			session("mango", "mango notes", 2*time.Hour),
			session("apple", "Apple review", 5*time.Minute),
			session("banana", "banana split", 24*time.Hour),
		},
		Projects: []*viewmodel.Project{
			{ID: "harbor", Name: "harbor", Path: "/fixture/harbor", Conversations: []*viewmodel.Conversation{
				session("rigging", "Rigging", time.Hour),
				session("ballast", "Ballast", 4*time.Hour),
				session("deck", "deck plan", 2*time.Hour),
				keel,
			}},
			{ID: "atlas", Name: "Atlas", Path: "/fixture/atlas", Conversations: []*viewmodel.Conversation{
				session("index", "Index maps", 72*time.Hour),
				session("contour", "contour lines", 120*time.Hour),
			}},
			{ID: "cedar", Name: "cedar", Path: "/fixture/cedar", Conversations: []*viewmodel.Conversation{
				session("grain", "Grain", 10*time.Minute),
			}},
		},
		Workspace: &viewmodel.Workspace{},
	}
}

// sidebarNamed finds the sidebar's elements assistive technology names so,
// top to bottom.
func sidebarNamed(tt *ui.Tester, names ...string) []ui.AccessInfo {
	var out []ui.AccessInfo
	for _, info := range tt.AccessAll() {
		if info.Bounds.X >= RailWidth && info.Bounds.X+info.Bounds.W <= RailWidth+NavWidth && slices.Contains(names, info.Name) {
			out = append(out, info)
		}
	}
	slices.SortStableFunc(out, func(a, b ui.AccessInfo) int { return cmp.Compare(a.Bounds.Y, b.Bounds.Y) })
	return out
}

// The groups: an empty one is its header alone, there is no Recent group,
// the header actions show without the pointer, and the sort menu orders the
// projects and the sessions, pinned ones staying first, and is kept.
func testSidebarGroups(t *testing.T) {
	zh := func(key string) string { return i18n.In("zh-CN", key) }
	open := func(a *App) *ui.Tester {
		a.Mode = theme.Dark
		tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
		tt.SetLocale("zh-CN")
		tt.Move(900, 600) // away from the sidebar: nothing in it is hovered
		settle(tt)
		return tt
	}
	// drawn reports whether something is painted in a control's box: the
	// icon of a button that is not hidden.
	drawn := func(tt *ui.Tester, info ui.AccessInfo) bool {
		img := tt.Image()
		scale := float32(img.Bounds().Dx()) / 1280
		b := info.Bounds
		x0, y0 := int(b.X*scale), int(b.Y*scale)
		corner := img.At(x0, y0)
		for y := y0; y < int((b.Y+b.H)*scale); y++ {
			for x := x0; x < int((b.X+b.W)*scale); x++ {
				if img.At(x, y) != corner {
					return true
				}
			}
		}
		return false
	}
	actions := func(tt *ui.Tester) (chat, sort, add ui.AccessInfo) {
		t.Helper()
		found := sidebarNamed(tt, zh("workspace_new_session"), zh("sidebar_sort"), zh("project_add"))
		if len(found) != 3 {
			t.Fatalf("the header actions are %+v", found)
		}
		// The chats' header is above the projects'; in it, sort is left of add.
		chat, sort, add = found[0], found[1], found[2]
		if sort.Bounds.X > add.Bounds.X {
			sort, add = add, sort
		}
		if chat.Name != zh("workspace_new_session") || sort.Name != zh("sidebar_sort") || add.Name != zh("project_add") {
			t.Fatalf("the header actions are %q, %q, %q", chat.Name, sort.Name, add.Name)
		}
		return chat, sort, add
	}

	// A fresh profile: both groups are their headers alone.
	empty := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
	tt := open(empty)
	snapshot(t, tt, "sidebar-empty")
	for _, key := range []string{"project_sessions_empty", "connection_no_projects", "recent_sessions"} {
		if tt.HasText(zh(key)) {
			t.Errorf("the empty sidebar shows %q: %q", zh(key), tt.Texts())
		}
	}
	if !tt.HasText(zh("sessions_unassigned")) || !tt.HasText(zh("projects")) {
		t.Fatalf("the empty sidebar lost a group header: %q", tt.Texts())
	}
	chat, sort, add := actions(tt)
	for _, b := range []ui.AccessInfo{chat, sort, add} {
		if b.Bounds.W != 20 || b.Bounds.H != 20 || !drawn(tt, b) {
			t.Errorf("%q is %vx%v, drawn %v, without the pointer", b.Name, b.Bounds.W, b.Bounds.H, drawn(tt, b))
		}
	}
	if chat.Bounds.X != add.Bounds.X || add.Bounds.X-sort.Bounds.X-sort.Bounds.W != 8 || sort.Bounds.Y != add.Bounds.Y {
		t.Errorf("the header actions are not in line: chat %+v, sort %+v, add %+v", chat.Bounds, sort.Bounds, add.Bounds)
	}

	// Two projects with sessions, for the look.
	two := groupsData()
	two.Projects = two.Projects[:2]
	tt = open(NewLive(two))
	snapshot(t, tt, "sidebar-projects")
	if tt.HasText(zh("recent_sessions")) {
		t.Errorf("the Recent group shows: %q", tt.Texts())
	}
	// The header actions end where a row's buttons do: 8 from its edge.
	chat, _, add = actions(tt)
	row := sidebarNamed(tt, "Apple review")[0].Bounds
	for _, b := range []ui.AccessInfo{chat, add} {
		if b.Bounds.X+b.Bounds.W != row.X+row.W-8 || b.Bounds.W != 20 || b.Bounds.H != 20 || !drawn(tt, b) {
			t.Errorf("%q %+v does not end 8 from the row's edge %+v", b.Name, b.Bounds, row)
		}
	}

	// Each order, chosen in the menu: the pinned session stays first, then
	// the chats, then each project with its sessions.
	a := NewLive(groupsData())
	tt = open(a)
	names := []string{"Keel survey", "mango notes", "Apple review", "banana split", "harbor", "Rigging", "Ballast", "deck plan", "Atlas", "Index maps", "contour lines", "cedar", "Grain"}
	rows := func() []string {
		var out []string
		for _, info := range sidebarNamed(tt, names...) {
			out = append(out, info.Name)
		}
		return out
	}
	choose := func(key string) {
		t.Helper()
		tt.Move(900, 600)
		_, sort, _ := actions(tt)
		tt.ClickAt(sort.Bounds.X+sort.Bounds.W/2, sort.Bounds.Y+sort.Bounds.H/2)
		settle(tt)
		if key == "" {
			return
		}
		clickItem(t, tt, zh(key))
		settle(tt)
	}
	updated := []string{"Keel survey", "Apple review", "mango notes", "banana split", "cedar", "Grain", "harbor", "Rigging", "deck plan", "Ballast", "Keel survey", "Atlas", "Index maps", "contour lines"}
	if got := rows(); !slices.Equal(got, updated) || a.Settings.Model.SessionOrder != int(sidebar.SortUpdated) {
		t.Fatalf("the default order, recently updated, is %d: %q", a.Settings.Model.SessionOrder, got)
	}
	choose("")
	snapshot(t, tt, "sidebar-sort-menu")
	var items []string
	for _, info := range tt.AccessAll() {
		if info.Role == ui.RoleMenuItem || info.Role == ui.RoleMenuItemCheckBox || info.Role == ui.RoleMenuItemRadio {
			items = append(items, fmt.Sprintf("%s %v", info.Name, info.Checked))
		}
	}
	if want := []string{zh("session_order_recent") + " true", zh("session_order_created") + " false", zh("session_order_name") + " false"}; !slices.Equal(items, want) {
		t.Fatalf("the sort menu is %q, want %q", items, want)
	}
	tt.Key(0, ui.KeyEscape)
	settle(tt)
	for _, c := range []struct {
		key  string
		sort sidebar.Sort
		want []string
	}{
		{"session_order_created", sidebar.SortCreated, []string{"Keel survey", "mango notes", "Apple review", "banana split", "cedar", "Grain", "Atlas", "Index maps", "contour lines", "harbor", "Rigging", "Ballast", "deck plan", "Keel survey"}},
		{"session_order_name", sidebar.SortName, []string{"Keel survey", "Apple review", "banana split", "mango notes", "Atlas", "contour lines", "Index maps", "cedar", "Grain", "harbor", "Ballast", "deck plan", "Keel survey", "Rigging"}},
		{"session_order_recent", sidebar.SortUpdated, updated},
	} {
		choose(c.key)
		if got := rows(); !slices.Equal(got, c.want) || a.Settings.Model.SessionOrder != int(c.sort) {
			t.Errorf("%s gives order %d:\n got %q\nwant %q", c.key, a.Settings.Model.SessionOrder, got, c.want)
		}
		// Next and previous conversation follow the rows.
		if got, want := a.Sidebar.Order(), map[sidebar.Sort][]string{
			sidebar.SortCreated: {"keel", "mango", "apple", "banana", "grain", "index", "contour", "rigging", "ballast", "deck"},
			sidebar.SortName:    {"keel", "apple", "banana", "mango", "contour", "index", "grain", "ballast", "deck", "rigging"},
			sidebar.SortUpdated: {"keel", "apple", "mango", "banana", "grain", "rigging", "deck", "ballast", "index", "contour"},
		}[c.sort]; !slices.Equal(got, want) {
			t.Errorf("%s: next and previous go %q, want %q", c.key, got, want)
		}
	}

	// The preview keeps the choice in the UI state file across a relaunch;
	// a live window leaves the preview's alone, its profile keeping its own.
	p, _ := newTester(t)
	if p.Settings.Model.SessionOrder != int(sidebar.SortUpdated) {
		t.Fatalf("the preview starts in order %d", p.Settings.Model.SessionOrder)
	}
	p.Settings.Model.SessionOrder = int(sidebar.SortName)
	dir := t.TempDir()
	if err := SavePrefs(dir, p.Prefs()); err != nil {
		t.Fatal(err)
	}
	again := New(preview.Load())
	again.SetPrefs(LoadPrefs(dir))
	if got := again.Sidebar.Order(); again.Settings.Model.SessionOrder != int(sidebar.SortName) || !slices.Equal(got, p.Sidebar.Order()) ||
		slices.Equal(got, New(preview.Load()).Sidebar.Order()) {
		t.Errorf("the relaunched preview is in order %d: %q", again.Settings.Model.SessionOrder, got)
	}
	live := NewLive(groupsData())
	live.SetPrefs(LoadPrefs(dir))
	if live.Settings.Model.SessionOrder != int(sidebar.SortUpdated) || live.Prefs().SessionOrder != "name" {
		t.Errorf("a live window took the preview's order: %d, wrote back %q", live.Settings.Model.SessionOrder, live.Prefs().SessionOrder)
	}

	// A UI state file from before: no order, and the names of groups that
	// are gone. It loads, keeping what it has.
	old := t.TempDir()
	state := `{"navWidth": 300, "navOpen": true, "panelWidth": 420, "collapsed": ["recent", "notes"], "recent": {"open": true}, "recentOpen": false}`
	if err := os.WriteFile(filepath.Join(old, PrefsFile), []byte(state), 0o644); err != nil {
		t.Fatal(err)
	}
	loaded := LoadPrefs(old)
	b := New(preview.Load())
	b.SetPrefs(loaded)
	bt := ui.NewTesterWith(b.View, 1280, 820, opaque)
	bt.SetLocale("en")
	bt.Frame()
	if loaded.NavWidth != 300 || b.NavWidth != 300 || b.Settings.Model.SessionOrder != int(sidebar.SortUpdated) {
		t.Errorf("the old UI state loaded as %+v, order %d", loaded, b.Settings.Model.SessionOrder)
	}
	if got := b.Sidebar.Collapsed(); !slices.Equal(got, []string{"notes"}) || bt.HasText(i18n.In("en", "recent_sessions")) {
		t.Errorf("the old UI state collapsed %q, Recent shows %v", got, bt.HasText(i18n.In("en", "recent_sessions")))
	}
}

// newSession finds the sidebar's New session button and the Chats group's
// "+", which do the same.
func newSession(t *testing.T, tt *ui.Tester) (button, plus ui.AccessInfo) {
	t.Helper()
	var found, added bool
	for _, info := range tt.AccessAll() {
		if info.Role != ui.RoleButton || info.Bounds.X < RailWidth || info.Bounds.X >= RailWidth+NavWidth {
			continue
		}
		switch info.Name {
		case i18n.In("en", "conversation"):
			if !found {
				button, found = info, true
			}
		case i18n.In("en", "workspace_new_session"):
			plus, added = info, true
		}
	}
	if !found || !added {
		t.Fatalf("no New session buttons (%v, %v): %q", found, added, tt.Texts())
	}
	return button, plus
}

// New session opens the one unsent chat and never stacks another. It is
// always enabled: while the page already shows that chat its click puts the
// focus back in the draft's editor.
func testSidebarNewSession(t *testing.T) {
	a, tt := newTester(t)
	press := func() {
		t.Helper()
		b, _ := newSession(t, tt)
		tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		settle(tt)
	}
	disabled := func() (bool, bool) {
		t.Helper()
		b, plus := newSession(t, tt)
		return b.Disabled, plus.Disabled
	}
	count := func() int { return len(a.Data.Unassigned) }
	before, selected := count(), a.Sidebar.Selected
	if b, plus := disabled(); b || plus {
		t.Fatalf("New session starts disabled: %v %v", b, plus)
	}
	press()
	draft := a.Sidebar.Selected
	if draft == selected || count() != before+1 || !tt.HasText(i18n.In("en", "welcome_title")) {
		t.Fatalf("New session opened %q among %d chats", draft, count())
	}
	editor := i18n.In("en", "composer")
	if b, plus := disabled(); b || plus || !tt.Focused(editor) {
		t.Fatalf("over the unsent chat New session is disabled %v, its + %v, the editor focused %v", b, plus, tt.Focused(editor))
	}
	// The focus leaves the editor; New session over the same draft brings
	// it back, as does the Chats group's "+".
	for _, again := range []string{"button", "plus"} {
		if err := tt.Click(i18n.In("en", "details")); err != nil {
			t.Fatal(err)
		}
		settle(tt)
		if tt.Focused(editor) {
			t.Fatalf("%s: the details toggle left the focus in the editor", again)
		}
		b, plus := newSession(t, tt)
		if again == "plus" {
			b = plus
		}
		tt.ClickAt(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		settle(tt)
		if !tt.Focused(editor) || a.Sidebar.Selected != draft || count() != before+1 {
			t.Fatalf("%s: New session over the draft focused the editor %v, selected %q among %d", again, tt.Focused(editor), a.Sidebar.Selected, count())
		}
	}
	snapshot(t, tt, "new-session-refocus")
	tt.Key(ui.Cmd, ui.KeyN)
	settle(tt)
	if a.Sidebar.Selected != draft || count() != before+1 {
		t.Fatalf("a second New session stacked a chat: %q among %d", a.Sidebar.Selected, count())
	}
	// From another session it is enabled and goes back to the unsent chat.
	click(t, tt, "Port the neutral theme tokens")
	settle(tt)
	if b, plus := disabled(); b || plus || a.Sidebar.Selected == draft {
		t.Fatalf("over another session New session is disabled %v %v", b, plus)
	}
	press()
	if a.Sidebar.Selected != draft || count() != before+1 {
		t.Fatalf("New session did not reuse the unsent chat: %q among %d", a.Sidebar.Selected, count())
	}
	// Beside a host's page the sessions stay: New session and the selected
	// session's own row both show the conversation.
	for _, back := range []struct {
		name string
		do   func()
	}{
		{"New session", press},
		{"the selected row", func() {
			a.Sidebar.Selected = "sailry-go/theme"
			a.openHost("remote", false)
			settle(tt)
			click(t, tt, "Port the neutral theme tokens")
			settle(tt)
		}},
	} {
		a.openHost("remote", false)
		settle(tt)
		if b, plus := disabled(); a.Page != HostPage || b || plus {
			t.Fatalf("%s: beside the host page New session is disabled %v %v on page %d", back.name, b, plus, a.Page)
		}
		back.do()
		if a.Page != ConversationPage {
			t.Fatalf("%s left the host page %d showing", back.name, a.Page)
		}
	}
	// Once something is sent in it, the chat is a session like the others
	// and New session opens another.
	a.Sidebar.Selected = draft
	a.Data.Find(draft).Title = "Sent"
	settle(tt)
	if b, plus := disabled(); b || plus {
		t.Fatalf("after sending New session is disabled %v %v", b, plus)
	}
	press()
	if a.Sidebar.Selected == draft || count() != before+2 {
		t.Fatalf("New session after sending opened %q among %d", a.Sidebar.Selected, count())
	}

	// Live: the runtime owns the drafts, reuses the one of a location and
	// keeps them out of the sidebar's list; a saved session without a title
	// is not one.
	live := NewLive(&viewmodel.Data{
		Unassigned: []*viewmodel.Conversation{{ID: "saved"}},
		Workspace:  &viewmodel.Workspace{},
	})
	drafts := 0
	live.Sidebar.Actions = &sidebar.Actions{NewDraft: func(*viewmodel.Project) {
		if drafts == 0 {
			drafts++
			cv := &viewmodel.Conversation{ID: fmt.Sprintf("draft/%d", drafts)}
			live.Data.Unassigned = append([]*viewmodel.Conversation{cv}, live.Data.Unassigned...)
			listed := *live.Data
			listed.Unassigned = live.Data.Unassigned[1:]
			live.Sidebar.Data = &listed
		}
		live.Page, live.Sidebar.Selected = ConversationPage, "draft/1"
	}}
	live.Sidebar.Selected = "saved"
	a, tt = live, ui.NewTesterWith(live.View, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	if b, plus := disabled(); b || plus || a.Sidebar.Selected != "saved" {
		t.Fatalf("over a saved untitled session New session is disabled %v %v", b, plus)
	}
	press()
	if drafts != 1 || a.Sidebar.Selected != "draft/1" {
		t.Fatalf("live New session made %d drafts, selected %q", drafts, a.Sidebar.Selected)
	}
	if b, plus := disabled(); b || plus || !tt.Focused(editor) {
		t.Fatalf("over the live draft New session is disabled %v, its + %v, the editor focused %v", b, plus, tt.Focused(editor))
	}
	if err := tt.Click(i18n.In("en", "details")); err != nil {
		t.Fatal(err)
	}
	settle(tt)
	if tt.Focused(editor) {
		t.Fatal("the details toggle left the focus in the live draft's editor")
	}
	press()
	if !tt.Focused(editor) {
		t.Fatal("New session over the live draft did not focus its editor")
	}
	a.Do("conversation.new")
	settle(tt)
	if drafts != 1 || count() != 2 || len(a.Sidebar.Data.Unassigned) != 1 {
		t.Fatalf("live New session stacked drafts: %d, %d chats, %d listed", drafts, count(), len(a.Sidebar.Data.Unassigned))
	}
}

func testSidebarMenuPinsAndRenames(t *testing.T) {
	a, tt := newTester(t)
	title := "Port the neutral theme tokens"
	if err := tt.RightClick(title); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem("Pin session"); err != nil {
		t.Fatalf("%v: %q", err, tt.Menu())
	}
	tt.Frame()
	cv := a.Data.Find("sailry-go/theme")
	if !cv.Pinned || cv.PinOrder == nil || *cv.PinOrder != 2 {
		t.Fatalf("the pin was not appended: %v %v", cv.Pinned, cv.PinOrder)
	}
	// Rename in place: the menu opens an input with the title.
	if err := tt.RightClick(title); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem("Rename"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyA)
	tt.Type("Theme tokens")
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if cv.Title != "Theme tokens" {
		t.Errorf("rename gave %q", cv.Title)
	}
}

func testSidebarRenameKeepsEmptyTitleOut(t *testing.T) {
	a, tt := newTester(t)
	cv := a.Data.Find("chat/regex")
	a.Sidebar.StartRename(cv)
	tt.Frame()
	tt.Key(ui.Cmd, ui.KeyA)
	tt.Key(0, ui.KeyBackspace)
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	if cv.Title != "Regex for semantic versions" || !tt.HasText(i18n.In("en", "session_title_required")) {
		t.Errorf("an empty title was taken: %q", cv.Title)
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if !tt.HasText("Regex for semantic versions") {
		t.Error("Escape did not give the rename up")
	}
}

func testSidebarReordersPins(t *testing.T) {
	a, _ := newTester(t)
	order := func() []string {
		var out []string
		for _, id := range a.Sidebar.Order()[:3] {
			out = append(out, id)
		}
		return out
	}
	before := order()
	a.Sidebar.MovePin(before[0], before[2], true)
	after := order()
	if after[2] != before[0] || after[0] != before[1] {
		t.Errorf("pins %v became %v", before, after)
	}
}

func TestProjectEditorAndRemoval(t *testing.T) {
	a, tt := newTester(t)
	en := func(key string, args ...any) string { return i18n.In("en", key, args...) }
	clickButton(t, tt, "Add project")
	tt.Frame()
	click(t, tt, "Save preview")
	if !tt.HasText(en("project_required")) {
		t.Fatalf("no validation toast: %q", tt.Texts())
	}
	n := len(a.Data.Projects)
	a.Sidebar.OpenEditor(nil)
	tt.Frame()
	clickField(t, tt, en("project_path"))
	tt.Type("~/Work/new-tool")
	tt.Frame()
	click(t, tt, "Save preview")
	tt.Frame()
	if len(a.Data.Projects) != n+1 || a.Data.Projects[n].Name != "new-tool" {
		t.Fatalf("the project was not added with its suggested name: %d", len(a.Data.Projects))
	}
	if pr := a.Data.Projects[n]; pr.Icon != "folder" || pr.Color != "none" {
		t.Fatalf("a new project's appearance is %q %q, want the folder without a color", pr.Icon, pr.Color)
	}

	// The appearance: the glyph button opens the colors and the icons.
	a.Sidebar.OpenEditor(nil)
	settle(tt)
	clickButton(t, tt, en("project_appearance"))
	settle(tt)
	for _, name := range []string{"project_color_magenta", "project_color_none", "project_icon_folder", "project_icon_favorite"} {
		if _, ok := tt.AccessByRole(ui.RoleButton, en(name)); !ok {
			t.Fatalf("the appearance popover lacks %s: %q", name, tt.Texts())
		}
	}
	clickButton(t, tt, en("project_color_green"))
	settle(tt)
	clickButton(t, tt, en("project_icon_code"))
	settle(tt)
	if d := a.Sidebar.Draft(); d.Icon != "code" || d.Color != "green" {
		t.Fatalf("the draft's appearance is %q %q", d.Icon, d.Color)
	}
	tt.Key(0, ui.KeyEscape)
	settle(tt)

	// The directory picker lists the host's sample directories: navigating,
	// filtering and the hidden toggle, then the choice fills the path.
	pick := func(name string) {
		t.Helper()
		all := tt.AccessAll()
		for i := len(all) - 1; i >= 0; i-- {
			if all[i].Role == ui.RoleButton && all[i].Name == name {
				b := all[i].Bounds
				tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
				settle(tt)
				return
			}
		}
		t.Fatalf("no button %q: %q", name, tt.Texts())
	}
	clickButton(t, tt, en("directory_title"))
	settle(tt)
	// Each is a whole label: the counts line is asserted exactly, with every
	// argument, so a changed wording or count fails here.
	for _, text := range []string{en("directory_preview"), "sailry", "scratch", "many", en("directory_locations"), en("directory_devices"),
		en("directory_counts_hidden", "directories", 8, "files", 11, "hidden", 1), "/Users/alex"} {
		if !slices.Contains(tt.Texts(), text) {
			t.Fatalf("the picker does not show %q: %q", text, tt.Texts())
		}
	}
	clickField(t, tt, en("directory_filter"))
	tt.Type("zz")
	settle(tt)
	if !tt.HasText(en("directory_no_matches")) || tt.HasText("scratch") {
		t.Fatalf("the filter kept unmatched entries: %q", tt.Texts())
	}
	pick(en("directory_filter")) // the clear button
	if !tt.HasText("scratch") {
		t.Fatalf("clearing the filter did not restore the entries: %q", tt.Texts())
	}
	click(t, tt, "scratch")
	settle(tt)
	if !tt.HasText("/Users/alex/scratch") {
		t.Fatalf("selecting a directory does not show its path: %q", tt.Texts())
	}
	tt.Key(0, ui.KeyEnter)
	settle(tt)
	if !tt.HasText("new-project") || tt.HasText("many") {
		t.Fatalf("Enter did not open the directory: %q", tt.Texts())
	}
	tt.Key(0, ui.KeyBackspace)
	settle(tt)
	if !tt.HasText("many") {
		t.Fatalf("Backspace did not go up: %q", tt.Texts())
	}
	pick(en("directory_back"))
	if !tt.HasText("new-project") {
		t.Fatalf("Back did not return: %q", tt.Texts())
	}
	// A new folder is made in memory through the native menu, and is hidden
	// until the toggle shows it.
	if err := tt.RightClick("new-project"); err != nil {
		t.Fatal(err)
	}
	if got := tt.Menu(); !slices.Contains(got, en("directory_open")) || !slices.Contains(got, en("files_rename")) || !slices.Contains(got, en("directory_trash")) {
		t.Fatalf("the entry menu is %q", got)
	}
	tt.CloseMenu()
	settle(tt)
	a.Sidebar.Picker.Create(".cache")
	settle(tt)
	if tt.HasText(".cache") || !slices.Contains(tt.Texts(), en("directory_counts_hidden", "directories", 1, "files", 1, "hidden", 1)) {
		t.Fatalf("a hidden entry shows before the toggle: %q", tt.Texts())
	}
	// The hidden-files toggle is an item of the header's More menu.
	more := tt.AccessAll()
	slices.Reverse(more)
	i := slices.IndexFunc(more, func(e ui.AccessInfo) bool {
		return (e.Role == ui.RoleMenuButton || e.Role == ui.RoleButton) && e.Name == en("more")
	})
	if i < 0 {
		t.Fatalf("the picker has no More menu: %q", tt.Texts())
	}
	tt.ClickAt(more[i].Bounds.X+more[i].Bounds.W/2, more[i].Bounds.Y+more[i].Bounds.H/2)
	settle(tt)
	clickItem(t, tt, en("directory_hidden"))
	settle(tt)
	if !tt.HasText(".cache") || !a.Sidebar.Picker.Hidden {
		t.Fatalf("the hidden toggle did not show the entry: %q", tt.Texts())
	}
	pick(en("directory_grid"))
	if !a.Sidebar.Picker.Grid || !tt.HasText("new-project") || tt.HasText(en("directory_kind")) {
		t.Fatalf("the grid view did not replace the table: %q", tt.Texts())
	}
	pick(en("directory_list"))
	click(t, tt, "new-project")
	settle(tt)
	snapshot(t, tt, "project-picker")
	pick(en("directory_title"))
	if d := a.Sidebar.Draft(); d.Path != "/Users/alex/scratch/new-project" || d.Name != "new-project" || a.Sidebar.Picker.Showing() {
		t.Fatalf("the choice did not fill the path: %+v", d)
	}

	// Clone keeps its own folder, takes a repository and a branch, and names
	// the project after the repository.
	click(t, tt, en("project_clone"))
	settle(tt)
	if d := a.Sidebar.Draft(); d.Path != "" {
		t.Fatalf("Clone kept the local path %q", d.Path)
	}
	clickField(t, tt, en("project_repository"))
	tt.Type("https://github.com/org/tool.git")
	clickField(t, tt, en("project_branch"))
	tt.Type("release")
	clickField(t, tt, en("project_destination"))
	tt.Type("~/src")
	settle(tt)
	if d := a.Sidebar.Draft(); d.Name != "tool" || !tt.HasText("~/src/tool") {
		t.Fatalf("the clone is named %q: %q", d.Name, tt.Texts())
	}
	if _, ok := tt.AccessByRole(ui.RoleButton, en("project_clone")); !ok {
		t.Fatalf("the clone's save button is not Clone: %q", tt.Texts())
	}
	snapshot(t, tt, "project-editor-clone")
	click(t, tt, en("project_local"))
	settle(tt)
	if d := a.Sidebar.Draft(); d.Path != "/Users/alex/scratch/new-project" {
		t.Fatalf("Local lost its path: %q", d.Path)
	}
	click(t, tt, "Save preview")
	settle(tt)
	added := a.Data.Projects[len(a.Data.Projects)-1]
	if len(a.Data.Projects) != n+2 || added.Icon != "code" || added.Color != "green" || added.Path != "/Users/alex/scratch/new-project" {
		t.Fatalf("the project was not saved with its appearance: %+v", added)
	}
	if !slices.Contains(tt.Texts(), en("project_icon_code")) {
		t.Fatalf("the sidebar row does not show the project's glyph")
	}

	// Editing keeps the host and changes the name and the appearance.
	if err := tt.RightClick("new-project"); err != nil {
		t.Fatal(err)
	}
	menu := tt.Menu()
	want := []string{"workspace_project", "workspace_new_session", "workspace_new_terminal", "workspace_records", "project_edit", "project_trust",
		"workspace_copy_path", "worktree_manage", "project_remove", "cli_launch"}
	at := 0
	for _, key := range want {
		i := slices.Index(menu[at:], en(key))
		if i < 0 {
			t.Fatalf("the project menu lacks %s in order: %q", key, menu)
		}
		at += i
	}
	if err := tt.ChooseMenuItem(en("project_edit")); err != nil {
		t.Fatal(err)
	}
	settle(tt)
	if !tt.HasText(en("project_edit")) || tt.HasText(en("project_clone")) {
		t.Fatalf("editing shows the new project's source: %q", tt.Texts())
	}
	clickButton(t, tt, en("project_appearance"))
	settle(tt)
	clickButton(t, tt, en("project_color_red"))
	settle(tt)
	tt.Key(0, ui.KeyEscape)
	settle(tt)
	click(t, tt, "Save preview")
	settle(tt)
	if added.Color != "red" || added.Icon != "code" || len(a.Data.Projects) != n+2 {
		t.Fatalf("the edit did not change the appearance: %+v", added)
	}
	snapshot(t, tt, "project-sidebar")

	// Trust is the preview's record alone, asked first.
	if err := tt.RightClick("new-project"); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem(en("project_trust")); err != nil {
		t.Fatal(err)
	}
	settle(tt)
	clickButton(t, tt, en("project_grant"))
	settle(tt)
	if !added.Trusted {
		t.Fatal("granting trust did not record it")
	}

	// Open folder opens the project already at the chosen directory, or adds
	// one named after it.
	a.Do("folder.open")
	settle(tt)
	if !tt.HasText(en("menu_open_folder")) || !a.Sidebar.Picker.Showing() {
		t.Fatalf("Open folder did not open the picker: %q", tt.Texts())
	}
	click(t, tt, "sailry")
	settle(tt)
	pick(en("directory_open"))
	if last := a.Data.Projects[len(a.Data.Projects)-1]; len(a.Data.Projects) != n+3 || last.Name != "sailry" || last.Path != "/Users/alex/sailry" || a.Sidebar.Picker.Showing() {
		t.Fatalf("Open folder did not add the project: %+v", last)
	}
	a.Do("folder.open")
	settle(tt)
	click(t, tt, "sailry")
	settle(tt)
	pick(en("directory_open"))
	if len(a.Data.Projects) != n+3 || a.Sidebar.Picker.Showing() {
		t.Fatalf("Open folder added a registered directory again: %d", len(a.Data.Projects))
	}

	// A project its sessions still use stays; an unused one is removed after
	// the confirmation names it.
	var all []string
	for _, pr := range a.Data.Projects {
		all = append(all, pr.ID)
	}
	a.Sidebar.SetCollapsed(all)
	settle(tt)
	tt.Advance(time.Second)
	settle(tt)
	if err := tt.RightClick("field-notes"); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem("Remove project"); err != nil {
		t.Fatal(err)
	}
	settle(tt)
	if !tt.HasText(en("project_remove_sessions")) || len(a.Data.Projects) != n+3 {
		t.Fatalf("a project with sessions was offered for removal: %q", tt.Texts())
	}
	if err := tt.RightClick("new-tool"); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem("Remove project"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	msg := en("project_remove_sample", "name", "~/Work/new-tool")
	if !tt.HasText(msg) {
		t.Fatalf("the confirmation does not name the project: %q", tt.Texts())
	}
	click(t, tt, en("project_remove_preview"))
	tt.Frame()
	if len(a.Data.Projects) != n+2 {
		t.Error("the project was not removed")
	}

	// The picker's frames for review, under SAILRY_SNAPSHOTS: over the
	// editor that opened it, in Chinese with the dialog's own frost, dark and
	// light, and a host that did not answer.
	t.Run("picker frames", func(t *testing.T) {
		if os.Getenv("SAILRY_SNAPSHOTS") == "" {
			t.Skip("SAILRY_SNAPSHOTS is not set")
		}
		zh := func(key string, args ...any) string { return i18n.In("zh-CN", key, args...) }
		open := func(dark bool) (*App, *ui.Tester) {
			a := New(preview.Load())
			tt := ui.NewTesterWith(a.View, 1280, 820, ui.TesterOptions{Dark: dark})
			tt.SetScale(2)
			tt.SetLocale("zh-CN")
			tt.Frame()
			// Two of the sample projects live in the sample tree, the
			// first used last.
			for i, path := range []string{"/Users/alex/sailry", "/Users/alex/scratch"} {
				a.Data.Projects[i].Host, a.Data.Projects[i].Path = a.Data.Hosts[0].ID, path
			}
			a.Sidebar.OpenEditor(nil)
			settle(tt)
			return a, tt
		}
		for _, dark := range []bool{true, false} {
			a, tt := open(dark)
			clickButton(t, tt, zh("directory_title"))
			settle(tt)
			click(t, tt, "scratch")
			tt.Move(4, 4)
			settle(tt)
			if !a.Sidebar.Picker.Showing() || !tt.HasText(zh("projects")) || !tt.HasText(zh("directory_modified")) {
				t.Fatalf("the picker's frame is incomplete: %q", tt.Texts())
			}
			snapshot(t, tt, "picker-zh-CN-"+map[bool]string{true: "dark", false: "light"}[dark])
		}
		a, tt := open(true)
		a.Sidebar.Picker.Open(picker.Options{Mode: picker.Directory, Title: "directory_title", Confirm: "directory_title", Initial: "/srv/builds",
			Host: "build-box", Remote: true, Projects: []picker.Location{{Name: "harbor-api", Path: "/srv/harbor-api"}, {Name: "lumen-web", Path: "/srv/lumen-web"}},
			Source: picker.Source{Browse: func(_ string, done func(picker.Listing, string)) {
				done(picker.Listing{}, "directory_host_unavailable")
			}}})
		settle(tt)
		settle(tt)
		if !tt.HasText(zh("directory_host_unavailable")) || !tt.HasText(zh("settings_retry")) {
			t.Fatalf("the failed picker's frame is incomplete: %q", tt.Texts())
		}
		snapshot(t, tt, "picker-error-zh-CN-dark")
	})
}

// clickField clicks the text field assistive technology names so.
func clickField(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	info, ok := tt.AccessByRole(ui.RoleTextField, name)
	if !ok {
		t.Fatalf("no field %q: %q", name, tt.Texts())
	}
	b := info.Bounds
	tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
}

// Delete asks first, naming the session once; confirming removes it.
func testSidebarDeleteConfirms(t *testing.T) {
	a, tt := newTester(t)
	title := "Port the neutral theme tokens"
	if err := tt.RightClick(title); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem(i18n.In("en", "session_delete")); err != nil {
		t.Fatalf("%v: %q", err, tt.Menu())
	}
	tt.Frame()
	if a.Data.Find("sailry-go/theme") == nil {
		t.Fatal("Delete removed the session before the confirmation")
	}
	if !tt.HasText(i18n.In("en", "session_delete_named", "name", title)) {
		t.Fatalf("the confirmation does not name the session: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "session_delete"))
	tt.Frame()
	if a.Data.Find("sailry-go/theme") != nil {
		t.Fatal("the session was not deleted")
	}
	if tt.HasText(title) {
		t.Errorf("the deleted session still shows: %q", tt.Texts())
	}
}
