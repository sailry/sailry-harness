package conversation_test

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type loginRecoveryActions struct {
	noActions
	turns []string
}

func (actions *loginRecoveryActions) ResumeLogin(turn string) {
	actions.turns = append(actions.turns, turn)
}

func TestControllerLoginRecoveryRemainsVisibleAfterCompletedTools(t *testing.T) {
	for _, language := range []string{"en", "zh-CN"} {
		t.Run(language, func(t *testing.T) {
			state, actions := conversation.New(), &loginRecoveryActions{}
			turns := []conversation.Turn{{ID: "waiting-task", User: "Continue the task", Reply: conversation.Reply{
				Status: conversation.Running, Phase: conversation.WaitingController, LoginRecovery: true,
				Blocks: []conversation.Block{{Kind: conversation.ToolsBlock, Calls: []conversation.Call{{ID: "done", Name: "read_file", Label: "Read file"}}}},
			}}}
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Children(func() { conversation.View(c, state, "session", turns, &conversation.Session{}, actions) })
			}, 800, 500)
			tester.SetLocale(language)
			tester.SetPreferences(ui.Preferences{ReduceMotion: true})
			tester.Frame()
			label := i18n.In(language, "controller_login_restore")
			button, ok := tester.AccessByRole(ui.RoleButton, label)
			if !ok || !tester.HasText(i18n.In(language, "turn_waiting_controller")) {
				t.Fatalf("waiting task lost its sign-in action after tools: %q", tester.Texts())
			}
			tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
			tester.Frame()
			if len(actions.turns) != 1 || actions.turns[0] != "waiting-task" {
				t.Fatalf("recovery targeted a different task: %v", actions.turns)
			}
			turns[0].Reply.Phase = conversation.Thinking
			tester.Frame()
			if _, exists := tester.AccessByRole(ui.RoleButton, label); exists {
				t.Fatal("sign-in recovery remained after the task left its waiting state")
			}
		})
	}
}
