package shell

import (
	"crypto/sha256"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/composer"
	"github.com/sailry/sailry-go/internal/conversation"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/settings"
	"github.com/sailry/sailry-go/internal/theme"
)

func newTester(t *testing.T) (*App, *ui.Tester) {
	t.Helper()
	a := New(preview.Load())
	tt := ui.NewTester(a.View, 1280, 820)
	tt.SetLocale("en")
	tt.Frame()
	return a, tt
}

func click(t *testing.T, tt *ui.Tester, label string) {
	t.Helper()
	if err := tt.Click(label); err != nil {
		t.Fatalf("click %q: %v\ntexts: %q", label, err, tt.Texts())
	}
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
	click(t, tt, "Select model")
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
	// The model list chooses another model, and the chip names it.
	clickButton(t, tt, "Claude Opus 5.5") // the panel's model, which opens the list
	clickButton(t, tt, "GPT-5.2")
	if comp.Model != "gpt-5.2" {
		t.Fatalf("model %q", comp.Model)
	}
	if !tt.HasText("GPT-5.2") || !tt.HasText("high") {
		t.Errorf("the chip does not show the choice: %q", tt.Texts())
	}
	// A model without reasoning shows the default effort, its slider off.
	// The picker stays open on its main panel after a choice.
	clickButton(t, tt, "GPT-5.2")
	clickButton(t, tt, "Claude Haiku 5.5")
	if !tt.HasText(i18n.In("en", "composer_effort_unavailable")) {
		t.Errorf("no default effort for a model without reasoning: %q", tt.Texts())
	}
}

func TestBranchMenu(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, "main")
	// The menu is kit.Menu, drawn in the window.
	for _, key := range composer.Branches {
		if !tt.HasText(i18n.In("en", key)) {
			t.Fatalf("branch menu lacks %q: %q", key, tt.Texts())
		}
	}
	click(t, tt, "preview/ui")
	tt.Frame()
	if a.Composer(preview.Big).Branch != 1 {
		t.Error("the branch was not chosen")
	}
}

func TestSettingsNavigation(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, "Settings")
	if a.Page != SettingsPage || a.Settings.Section != settings.Providers {
		t.Fatalf("page %d section %q", a.Page, a.Settings.Section)
	}
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
	if a.Settings.Section != settings.Plugins || !tt.HasText("Terminal") || !tt.HasText(i18n.In("en", "plugins_official")) {
		t.Fatalf("Plugins not shown: %q", tt.Texts())
	}
	snapshot(t, tt, "plugins-light")
	// A plugin's switch turns it off.
	plugin := &a.Data.Plugins[0]
	sw, ok := tt.AccessByRole(ui.RoleSwitch, i18n.In("en", "plugins_enabled")+" "+plugin.Name)
	if !ok || !sw.Checked {
		t.Fatalf("switch %+v", sw)
	}
	click(t, tt, i18n.In("en", "plugins_enabled")+" "+plugin.Name)
	if plugin.Enabled {
		t.Error("the switch did not turn the plugin off")
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
	tt := ui.NewTester(a.View, 1280, 820)
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

// Appearance applies a theme package from its card, shows its details and
// turns the effects off.
func TestAppearancePackages(t *testing.T) {
	a, tt := newTester(t)
	click(t, tt, "Settings")
	click(t, tt, i18n.In("en", "settings_appearance"))
	sage := theme.Find("great-sage")
	clickButton(t, tt, sage.Name)
	if a.Theme != sage.ID {
		t.Fatalf("theme %q after choosing %s", a.Theme, sage.Name)
	}
	if !tt.HasText(sage.Release) || !tt.HasText(sage.License.Author) {
		t.Errorf("no details of %s: %q", sage.Name, tt.Texts())
	}
	snapshot(t, tt, "appearance-great-sage")
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
