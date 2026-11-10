package shell

import (
	"crypto/sha256"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// opaque keeps the software shader interpreter out of every headless frame:
// frosted surfaces fall back to their opaque fill under Reduce Transparency.
var opaque = ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}}

func newTester(t *testing.T) (*App, *ui.Tester) {
	t.Helper()
	a := New(preview.Load())
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("en")
	tt.Frame()
	return a, tt
}

// clickTree clicks the file tree's row labelled label, not a tab of the
// same name.
func clickTree(t *testing.T, tt *ui.Tester, label string) {
	t.Helper()
	e, ok := tt.AccessByRole(ui.RoleTreeItem, label)
	if !ok {
		t.Fatalf("no tree row %q", label)
	}
	b := e.Bounds
	tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
}

// codeHas reports whether the editor or diff side labelled label shows s.
func codeHas(tt *ui.Tester, label, s string) bool {
	e, ok := tt.Access(label)
	return ok && strings.Contains(e.Value, s)
}

func click(t *testing.T, tt *ui.Tester, label string) {
	t.Helper()
	if err := tt.Click(label); err != nil {
		t.Fatalf("click %q: %v\ntexts: %q", label, err, tt.Texts())
	}
}

// clickItem clicks the control named name among the menu items and menu
// buttons, past texts of the same name elsewhere in the window.
func clickItem(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	for _, role := range []ui.Role{ui.RoleMenuItem, ui.RoleMenuItemCheckBox, ui.RoleMenuItemRadio, ui.RoleMenuButton} {
		if info, ok := tt.AccessByRole(role, name); ok {
			b := info.Bounds
			tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
			return
		}
	}
	t.Fatalf("no menu item %q: %q", name, tt.Texts())
}

// snapshot writes the last frame to $SAILRY_SNAPSHOTS/name.png, when set.
func snapshot(t *testing.T, tt *ui.Tester, name string) {
	dir := os.Getenv("SAILRY_SNAPSHOTS")
	if dir == "" {
		return
	}
	f, err := os.Create(filepath.Join(dir, name+".png"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tt.Image()); err != nil {
		t.Fatal(err)
	}
}

func TestSidebarOpensConversation(t *testing.T) {
	a, tt := newTester(t)
	snapshot(t, tt, "conversation-light")
	if !tt.HasText("Virtualized transcript stress test") {
		t.Fatalf("the large conversation is not shown: %q", tt.Texts())
	}
	click(t, tt, "Port the neutral theme tokens")
	if a.Sidebar.Selected != "sailry-go/theme" {
		t.Fatalf("selected %q", a.Sidebar.Selected)
	}
	// The header names it and the transcript shows its last message.
	cv := a.Data.Find("sailry-go/theme")
	if !tt.HasText(cv.Turns[len(cv.Turns)-1].User) {
		t.Errorf("the transcript of %q is not shown: %q", cv.Title, tt.Texts())
	}
	// A selected button reads as pressed.
	if info, ok := tt.AccessByRole(ui.RoleButton, cv.Title); !ok || !info.Checked {
		t.Errorf("the row is not selected for assistive technology: %+v", info)
	}
}

func TestSidebarShowMore(t *testing.T) {
	_, tt := newTester(t)
	hidden := "zsh completion errors" // the 6th chat without a project
	if tt.HasText(hidden) {
		t.Fatal("rows past the limit show before Show more")
	}
	click(t, tt, "Show more") // the chats' toggle comes first
	if !tt.HasText(hidden) || !tt.HasText("Show less") {
		t.Errorf("Show more did not show the rest: %q", tt.Texts())
	}
}

// clickButton clicks the button assistive technology names so, where a text
// would match another element first.
func clickButton(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	info, ok := tt.AccessByRole(ui.RoleButton, name)
	if !ok {
		t.Fatalf("no button %q: %q", name, tt.Texts())
	}
	b := info.Bounds
	tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
}

func TestComposerSend(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, "Port the neutral theme tokens")
	send, _ := tt.AccessByRole(ui.RoleButton, "Send")
	if !send.Disabled {
		t.Fatalf("Send is enabled with an empty draft: %+v", send)
	}
	click(t, tt, i18n.In("en", "composer"))
	tt.Type("Make the borders lighter")
	if send, _ = tt.AccessByRole(ui.RoleButton, "Send"); send.Disabled {
		t.Fatal("Send is disabled with a draft")
	}
	cv := a.Data.Find("sailry-go/theme")
	n := len(cv.Turns)
	click(t, tt, "Send")
	if len(cv.Turns) != n+1 || cv.Turns[n].User != "Make the borders lighter" {
		t.Fatalf("not sent: %d turns", len(cv.Turns))
	}
	if a.Composer(cv.ID).Draft != "" {
		t.Error("the draft stays after sending")
	}
	// The reply runs: Stop replaces Send until it is pressed.
	if _, ok := tt.AccessByRole(ui.RoleButton, "Stop"); !ok {
		t.Fatalf("no Stop while the reply runs: %q", tt.Texts())
	}
	tt.Advance(100 * time.Millisecond)
	snapshot(t, tt, "running")
	click(t, tt, "Stop")
	if _, ok := tt.AccessByRole(ui.RoleButton, "Send"); !ok {
		t.Error("Send does not come back after Stop")
	}
	if st := cv.Turns[n].Reply.Status; st != conversation.Cancelled {
		t.Errorf("stopped reply has status %d", st)
	}
	// Enter sends too.
	click(t, tt, i18n.In("en", "composer"))
	tt.Type("Again")
	tt.Key(0, ui.KeyEnter)
	if last := cv.Turns[len(cv.Turns)-1]; last.User != "Again" {
		t.Errorf("Enter did not send: %d turns, last %q", len(cv.Turns), last.User)
	}
}

// The sent reply plays to its end: the run completes and its answer shows.
func TestReplyPlays(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, "Port the neutral theme tokens")
	click(t, tt, i18n.In("en", "composer"))
	tt.Type("Hello")
	tt.Key(0, ui.KeyEnter)
	cv := a.Data.Find("sailry-go/theme")
	for range 200 {
		if !cv.Playing() {
			break
		}
		tt.Advance(preview.Tick)
	}
	r := cv.Turns[len(cv.Turns)-1].Reply
	if r.Status != conversation.Completed || cv.Status != preview.Idle {
		t.Fatalf("the reply did not complete: status %d", r.Status)
	}
	if !tt.HasText(i18n.In("en", "turn_completed")) {
		t.Errorf("no completed status: %q", tt.Texts())
	}
}

// The live reply's approvals and question wait in the composer's capsules;
// a decision there changes the preview's row and says so in a toast.
func TestApprovalsAndQuestion(t *testing.T) {
	a, tt := newTester(t)
	cv := a.Data.Find(preview.Big)
	if len(cv.Waiting.Approvals) != 2 {
		t.Fatalf("%d approvals", len(cv.Waiting.Approvals))
	}
	clickButton(t, tt, i18n.In("en", "approval_live_count", "count", 2))
	snapshot(t, tt, "approvals")
	// One request at a time, paged by its position.
	if !tt.HasText("1/2") || !tt.HasText("internal/kit/loader.go") {
		t.Fatalf("the approvals do not show the first request: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "knowledge_next"))
	if !tt.HasText("2/2") || !tt.HasText("go vet ./...") {
		t.Fatalf("next does not show the second request: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "knowledge_previous"))
	clickButton(t, tt, i18n.In("en", "approval_allow_once"))
	if len(cv.Waiting.Approvals) != 1 {
		t.Fatalf("the decision did not resolve the approval: %d left", len(cv.Waiting.Approvals))
	}
	if !tt.HasText(i18n.In("en", "approval_preview_result")) {
		t.Errorf("no preview toast: %q", tt.Texts())
	}
	// The question: choose an option and submit.
	clickButton(t, tt, i18n.In("en", "question_reply"))
	tt.Advance(time.Second) // the popover's entrance
	tt.Frame()
	// The popover sits above its capsule, right-aligned with it, 8 DIPs
	// apart (Anchor::BottomRight, bottom_2): Submit ends at Kit's popover
	// padding (12) and MyGo's border (1).
	capsule, _ := tt.AccessByRole(ui.RoleButton, i18n.In("en", "question_reply"))
	submit, _ := tt.AccessByRole(ui.RoleButton, i18n.In("en", "interaction_submit"))
	cb, sb := capsule.Bounds, submit.Bounds
	if d := (cb.X + cb.W) - (sb.X + sb.W); d < 12 || d > 14 {
		t.Errorf("the question popover is not right-aligned with its capsule: capsule %v, Submit %v", cb, sb)
	}
	if d := cb.Y - (sb.Y + sb.H); d < 20 || d > 22 {
		t.Errorf("the question popover is not 8 DIPs above its capsule: capsule %v, Submit %v", cb, sb)
	}
	click(t, tt, "Use the spinner")
	clickButton(t, tt, i18n.In("en", "interaction_submit"))
	if cv.Waiting.Question != nil {
		t.Fatal("the answer did not close the question")
	}
}

func TestModelPickerAndEffort(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, i18n.In("en", "composer_model_settings")+": Claude Opus 5.5")
	tt.Advance(time.Second) // the popover's entrance
	tt.Frame()
	snapshot(t, tt, "model-picker")
	if !tt.HasText("medium") {
		t.Fatalf("the picker does not show the effort: %q", tt.Texts())
	}
	// The effort slider steps with the arrows.
	slider, ok := tt.AccessByRole(ui.RoleSlider, "Reasoning effort")
	if !ok {
		t.Fatalf("no effort slider")
	}
	click(t, tt, "Reasoning effort")
	tt.Key(0, ui.KeyRight)
	comp := a.Composer(preview.Big)
	if got := composer.Efforts[int(comp.Effort)]; got != "high" {
		t.Errorf("effort %q after Right from medium (slider %+v)", got, slider)
	}
	// The model list chooses another model; a reasoning model takes its
	// effort from a submenu.
	clickButton(t, tt, "Claude Opus 5.5") // the panel's model, which opens the list
	// The list opens under the pointer; choosing a provider does not
	// depend on which row the pointer happens to rest on.
	clickButton(t, tt, i18n.In("en", "provider_openai"))
	clickItem(t, tt, "GPT-5.2")
	clickItem(t, tt, "high")
	tt.Frame()
	if comp.Model != "gpt-5.2" {
		t.Fatalf("model %q", comp.Model)
	}
	if got := composer.Efforts[int(comp.Effort)]; got != "high" {
		t.Errorf("effort %q after choosing high", got)
	}
	// A model without reasoning shows the default effort, its slider off.
	// The picker stays open on its main panel after a choice.
	clickButton(t, tt, "GPT-5.2")
	clickButton(t, tt, i18n.In("en", "provider_anthropic"))
	clickButton(t, tt, "Claude Haiku 5.5")
	if comp.Model != "claude-haiku-5-5" {
		t.Fatalf("model %q", comp.Model)
	}
	if !tt.HasText(i18n.In("en", "composer_effort_unavailable")) {
		t.Errorf("no default effort for a model without reasoning: %q", tt.Texts())
	}
}

func TestWorktreeMenu(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, "main")
	// The menu is kit.Menu, drawn in the window.
	for _, want := range []string{"sailry-go", "split-view", i18n.In("en", "location_create")} {
		if !tt.HasText(want) {
			t.Fatalf("worktree menu lacks %q: %q", want, tt.Texts())
		}
	}
	clickItem(t, tt, "split-view")
	tt.Frame()
	if w := a.Composer(preview.Big).Worktree; w != "split-view" {
		t.Errorf("worktree %q", w)
	}
}

func TestSettingsNavigation(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, "Settings")
	if a.Page != SettingsPage || a.Settings.Section != settings.General {
		t.Fatalf("page %d section %q", a.Page, a.Settings.Section)
	}
	click(t, tt, i18n.In("en", "settings_providers"))
	snapshot(t, tt, "providers-light")
	for _, want := range []string{"Anthropic", "OpenAI", i18n.In("en", "settings_providers_description")} {
		if !tt.HasText(want) {
			t.Errorf("Providers lacks %q", want)
		}
	}
	// The search filters the list.
	click(t, tt, i18n.In("en", "provider_list_search"))
	tt.Type("open")
	if tt.HasText("Anthropic") || !tt.HasText("OpenAI") && !tt.HasText("OpenRouter") {
		t.Errorf("search did not filter: %q", tt.Texts())
	}
	click(t, tt, i18n.In("en", "settings_plugins"))
	if a.Settings.Section != settings.Plugins || !tt.HasText("Terminal") || !tt.HasText(i18n.In("en", "plugins_installed")) {
		t.Fatalf("Plugins not shown: %q", tt.Texts())
	}
	snapshot(t, tt, "plugins-light")
	// A plugin's switch turns it off.
	plugin := &a.Settings.Packages[0]
	sw, ok := tt.AccessByRole(ui.RoleSwitch, i18n.In("en", "plugins_enabled")+" "+plugin.Name)
	if !ok || !sw.Checked {
		t.Fatalf("switch %+v", sw)
	}
	click(t, tt, i18n.In("en", "plugins_enabled")+" "+plugin.Name)
	if plugin.Enabled {
		t.Error("the switch did not turn the plugin off")
	}
	// A card opens the plugin's details in the right column; Close shuts it.
	click(t, tt, plugin.Description)
	tt.Advance(time.Second)
	if a.Settings.Details == nil || !tt.HasText(i18n.In("en", "market_plugin_details")) {
		t.Fatalf("details not shown: %+v", a.Settings.Details)
	}
	tt.Frame()
	snapshot(t, tt, "plugin-details-light")
	click(t, tt, i18n.In("en", "close"))
	tt.Advance(time.Second)
	if a.Settings.Details != nil {
		t.Error("Close kept the details")
	}
	// Back to the conversation, the rail's Chats.
	clickButton(t, tt, i18n.In("en", "shell_sessions"))
	if a.Page != ConversationPage {
		t.Error("the rail does not go back to conversations")
	}
}

func TestThemeSwitch(t *testing.T) {
	a, tt := newTester(t)
	bg := func() string {
		c := tt.Image().At(640, 600)
		r, g, b, _ := c.RGBA()
		return fmt.Sprint(r>>8, g>>8, b>>8)
	}
	light := bg()
	click(t, tt, "Settings")
	click(t, tt, i18n.In("en", "settings_appearance"))
	click(t, tt, i18n.In("en", "appearance_dark"))
	if a.Mode != theme.Dark {
		t.Fatalf("mode %d", a.Mode)
	}
	click(t, tt, i18n.In("en", "settings_providers"))
	snapshot(t, tt, "providers-dark")
	click(t, tt, i18n.In("en", "conversation"))
	snapshot(t, tt, "conversation-dark")
	if dark := bg(); dark == light {
		t.Errorf("the panel stays %s in dark mode", dark)
	}
	// System follows the desktop.
	a.Mode = theme.System
	tt.SetDark(true)
	tt.Frame()
	if bg() == light {
		t.Error("System does not follow the dark desktop")
	}
}

func TestChinese(t *testing.T) {
	a := New(preview.Load())
	tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
	tt.SetLocale("zh-CN")
	tt.Frame()
	for _, want := range []string{"新会话", i18n.In("zh-CN", "preview")} {
		if !tt.HasText(want) {
			t.Errorf("zh-CN lacks %q: %q", want, tt.Texts())
		}
	}
	snapshot(t, tt, "conversation-zh")
}

// newChat opens a new conversation through the sidebar's button, over the
// navigation at the top (the rail's button has the same name).
func newChat(t *testing.T, a *App, tt *ui.Tester) *preview.Conversation {
	t.Helper()
	before := a.Sidebar.Selected
	tt.ClickAt(RailWidth+NavWidth/2, HeaderHeight+32)
	cv := a.Data.Find(a.Sidebar.Selected)
	if cv == nil || cv.ID == before || len(cv.Turns) != 0 {
		t.Fatalf("no new conversation: %q", a.Sidebar.Selected)
	}
	return cv
}

// A new conversation welcomes; a mode card puts its prompt in the draft.
func TestWelcome(t *testing.T) {
	a, tt := newTester(t)
	cv := newChat(t, a, tt)
	if !tt.HasText(i18n.In("en", "welcome_title")) {
		t.Fatalf("no welcome: %q", tt.Texts())
	}
	clickButton(t, tt, i18n.In("en", "welcome_review"))
	if got, want := a.Composer(cv.ID).Draft, i18n.In("en", "welcome_review_prompt"); got != want {
		t.Errorf("draft %q, want the card's prompt", got)
	}
}

// Every shipped theme paints the transcript and the welcome page in both
// modes, each differently.
func TestThemePackages(t *testing.T) {
	seen := map[string]string{}
	for _, pkg := range theme.Packages {
		for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
			name := fmt.Sprintf("%s-%d", pkg.ID, mode)
			a, tt := newTester(t)
			a.Theme, a.Mode = pkg.ID, mode
			tt.Frame()
			snapshot(t, tt, "theme-"+name)
			sum := fmt.Sprintf("%x", sha256.Sum256(tt.Image().Pix))
			if other, ok := seen[sum]; ok {
				t.Errorf("%s paints as %s", name, other)
			}
			seen[sum] = name
			newChat(t, a, tt)
			tt.Advance(100 * time.Millisecond)
			snapshot(t, tt, "welcome-"+name)
			if !tt.HasText(i18n.In("en", "welcome_title")) {
				t.Errorf("%s: no welcome", name)
			}
		}
	}
}

// Appearance applies a theme package from its card, shows its details in
// a dialog from the card's right-click menu and turns the effects off.
func TestAppearancePackages(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, "Settings")
	click(t, tt, i18n.In("en", "settings_appearance"))
	sage := theme.Find("great-sage")
	clickButton(t, tt, sage.Name)
	if a.Theme != sage.ID {
		t.Fatalf("theme %q after choosing %s", a.Theme, sage.Name)
	}
	if tt.HasText(i18n.In("en", "appearance_details")) || tt.HasText(sage.Release) {
		t.Errorf("the page still lists the details: %q", tt.Texts())
	}
	if err := tt.RightClick(sage.Name); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem(i18n.In("en", "appearance_details")); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !tt.HasText(sage.Release) || !tt.HasText(sage.License.Author) || !tt.HasText(i18n.In("en", "appearance_information")) {
		t.Errorf("no details dialog of %s: %q", sage.Name, tt.Texts())
	}
	snapshot(t, tt, "appearance-great-sage")
	clickButton(t, tt, i18n.In("en", "close"))
	tt.Advance(time.Second)
	tt.Frame()
	clickButton(t, tt, i18n.In("en", "settings_theme_builtin"))
	if a.Theme != "" {
		t.Errorf("theme %q after choosing the default", a.Theme)
	}
	info, ok := tt.AccessByRole(ui.RoleSwitch, i18n.In("en", "appearance_effects"))
	if !ok {
		t.Fatalf("no effects switch: %q", tt.Texts())
	}
	tt.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
	if a.Effects {
		t.Error("the effects switch does not turn them off")
	}
}

// A file without a native preview opens no tab: Open hands it to the
// system's app, and a file that is not there toasts the failure.
func TestOpenUnsupportedFile(t *testing.T) {
	a := New(preview.Load())
	var x *actions
	f := conversation.File{ID: "x/pdf", Path: "exports/missing-invoice.pdf", Kind: conversation.OtherFile}
	opened := false
	tt := ui.NewTester(func(c *ui.Context) {
		a.View(c)
		if !opened {
			opened = true
			x = &actions{a: a, c: c}
			x.OpenFile(f)
		}
	}, 1280, 820)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	if a.Panel.Active.Kind == panel.Artifact {
		t.Errorf("an unsupported file opened a tab: %v", a.Panel.Tabs)
	}
	if !tt.HasText(i18n.In("en", "files_open_failed")) {
		t.Errorf("no failure toast: %q", tt.Texts())
	}
}

// Editing a queued message shows its files as the draft's cards, below
// the actions and at the text's inset, without the draft's Remove.
func TestQueueEditorShowsAttachments(t *testing.T) {
	_, tt := newTester(t)
	edit := i18n.In("en", "queue_edit")
	var last ui.AccessInfo
	for _, e := range tt.AccessAll() {
		if e.Role == ui.RoleButton && e.Name == edit && e.Bounds.Y > last.Bounds.Y {
			last = e
		}
	}
	if last.Name == "" {
		t.Fatal("no queued message to edit")
	}
	tt.ClickAt(last.Bounds.X+last.Bounds.W/2, last.Bounds.Y+last.Bounds.H/2)
	tt.Advance(time.Second)
	tt.Frame()
	snapshot(t, tt, "queue-editor")
	var input ui.AccessInfo
	for _, e := range tt.AccessAll() {
		if e.Name == edit && e.Multiline {
			input = e
		}
	}
	if input.Value != "Add the numbers to the report" {
		t.Fatalf("the editor does not show the message: %+v", input)
	}
	card, ok := tt.Access("benchmarks.md")
	if !ok {
		t.Fatalf("the editor does not show the message's file: %q", tt.Texts())
	}
	save, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "queue_save"))
	if !ok || card.Bounds.Y > save.Bounds.Y+save.Bounds.H || card.Bounds.Y+card.Bounds.H < save.Bounds.Y ||
		card.Bounds.X+card.Bounds.W > save.Bounds.X {
		t.Fatalf("the file %v is not on the actions' row before them %v", card.Bounds, save.Bounds)
	}
	// The text area pads its text by 4: the card starts where the text does.
	if d := card.Bounds.X - (input.Bounds.X + 4); d < -0.5 || d > 0.5 {
		t.Errorf("the file at x %v is not at the text's inset %v", card.Bounds.X, input.Bounds.X+4)
	}
	// The strip's edge fade leaves the card itself opaque: its name is
	// drawn in the foreground, not all but masked away.
	img, darkest := tt.Image(), uint8(255)
	for y := int(card.Bounds.Y); y < int(card.Bounds.Y+card.Bounds.H); y++ {
		for x := int(card.Bounds.X); x < int(card.Bounds.X+card.Bounds.W); x++ {
			darkest = min(darkest, img.RGBAAt(x, y).G)
		}
	}
	if darkest > 96 {
		t.Errorf("the file card is faded: its darkest pixel is %d", darkest)
	}
	tt.Move(card.Bounds.X+card.Bounds.W/2, card.Bounds.Y+card.Bounds.H/2)
	tt.Frame()
	if _, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "chat_attachment_remove")); ok {
		t.Error("a queued message's file offers Remove")
	}
}

// Tab queues behind the running reply; @ opens the references, a page
// opens on Enter and a row becomes a chip; Escape closes the picker
// before it stops the reply.
func TestComposerQueueReferencesEscape(t *testing.T) {
	a, tt := newTester(t)
	cv := a.Data.Find(preview.Big)
	comp := a.Composer(preview.Big)
	if !cv.Running() {
		t.Fatal("the big conversation does not run")
	}
	n := len(cv.Queue)
	click(t, tt, i18n.In("en", "composer"))
	tt.Type("Queue me")
	tt.Key(0, ui.KeyTab)
	tt.Frame()
	if len(cv.Queue) != n+1 || cv.Queue[n].Text != "Queue me" || comp.Draft != "" {
		t.Fatalf("Tab did not queue: %d queued, draft %q", len(cv.Queue), comp.Draft)
	}
	tt.Type("@")
	tt.Frame()
	if !tt.HasText(i18n.In("en", "reference_title")) {
		t.Fatalf("@ opens no picker: %q", tt.Texts())
	}
	tt.Key(0, ui.KeyDown) // Agents
	tt.Key(0, ui.KeyEnter)
	tt.Frame()
	tt.Key(0, ui.KeyEnter) // @reviewer
	tt.Frame()
	if comp.Draft != "@reviewer " || len(comp.References) != 1 {
		t.Fatalf("no chip: draft %q, references %v", comp.Draft, comp.References)
	}
	tt.Type(" @")
	tt.Frame()
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if tt.HasText(i18n.In("en", "reference_title")) || !cv.Running() {
		t.Fatalf("Escape did not close the picker alone (running %v)", cv.Running())
	}
	tt.Key(0, ui.KeyEscape)
	tt.Frame()
	if cv.Running() {
		t.Error("Escape with the picker closed does not stop the reply")
	}
}
