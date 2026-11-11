package shell

import (
	"fmt"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
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
	clickButton(t, tt, "Add project")
	tt.Frame()
	click(t, tt, "Save preview")
	if !tt.HasText(i18n.In("en", "project_required")) {
		t.Fatalf("no validation toast: %q", tt.Texts())
	}
	n := len(a.Data.Projects)
	a.Sidebar.OpenEditor(nil)
	tt.Frame()
	clickField(t, tt, i18n.In("en", "project_path"))
	tt.Type("~/Work/new-tool")
	tt.Frame()
	click(t, tt, "Save preview")
	tt.Frame()
	if len(a.Data.Projects) != n+1 || a.Data.Projects[n].Name != "new-tool" {
		t.Fatalf("the project was not added with its suggested name: %d", len(a.Data.Projects))
	}
	a.Sidebar.SetCollapsed([]string{"sailry-go", "sailry"})
	tt.Frame()
	tt.Advance(time.Second)
	tt.Frame()
	if err := tt.RightClick("field-notes"); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem("Remove project"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	msg := i18n.In("en", "project_remove_named", "name", "~/Documents/field-notes")
	if !tt.HasText(msg) {
		t.Fatalf("the confirmation does not name the project: %q", tt.Texts())
	}
	click(t, tt, i18n.In("en", "project_remove_preview"))
	tt.Frame()
	if len(a.Data.Projects) != n {
		t.Error("the project was not removed")
	}
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
