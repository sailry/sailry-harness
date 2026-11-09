package shell

import (
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/composer"
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
	// The header names it and the transcript shows its first message.
	cv := a.Data.Find("sailry-go/theme")
	if !tt.HasText(cv.Messages[len(cv.Messages)-2].Text) {
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
	n := len(cv.Messages)
	click(t, tt, "Send")
	if len(cv.Messages) != n+1 || cv.Messages[n].Text != "Make the borders lighter" {
		t.Fatalf("not sent: %d messages, last %q", len(cv.Messages), cv.Messages[len(cv.Messages)-1].Text)
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
	// Enter sends too; a turn stopped before its reply stays paired.
	click(t, tt, i18n.In("en", "composer"))
	tt.Type("Again")
	tt.Key(0, ui.KeyEnter)
	if last := cv.Messages[len(cv.Messages)-1]; last.Text != "Again" || len(cv.Messages)%2 != 1 {
		t.Errorf("Enter did not send: %d messages, last %q", len(cv.Messages), last.Text)
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
	if err := tt.ChooseMenuItem("preview/ui"); err != nil {
		t.Fatalf("%v: menu %q", err, tt.Menu())
	}
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
	// Back to the conversation.
	click(t, tt, i18n.In("en", "conversation"))
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

