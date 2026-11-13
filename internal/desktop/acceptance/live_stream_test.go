package acceptance

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLiveStreamSamplesEveryPaintedFrameDuringSettle(t *testing.T) {
	const input = "Describe a fixture kite"
	const first = "A red kite rises above a quiet field."
	const grown = first + " Its long tail follows the wind across the sky."
	const final = grown + " KITE-FINISHED"
	session, turn := protocol.SessionID{2}, protocol.TurnID{3}
	cv := &viewmodel.Conversation{ID: session.String(), Title: "Stream fixture", Session: conversation.Session{Connection: conversation.Connected}}
	w := &liveWindow{window: &window{ctx: t.Context(), app: shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})},
		session: protocol.Session{ID: session}, input: input}
	w.app.Effects, w.app.NavOpen, w.app.Sidebar.Selected = false, false, cv.ID
	phase, enabled := 0, false
	w.tester = ui.NewTesterWith(func(c *ui.Context) {
		if enabled {
			status := conversation.Running
			if phase == 2 {
				status = conversation.Completed
			}
			cv.Turns = []conversation.Turn{{ID: turn.String(), User: input, Reply: conversation.Reply{Status: status,
				Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: []string{first, grown, final}[phase]}}}}}
		}
		w.view(c)
		if enabled && phase < 2 {
			// Advance only after this frame paints. Multiple build passes must
			// not skip a fixture sample; one Frame() settles all three paints.
			advanced := false
			ui.Box(c.Key("advance-stream-fixture")).Absolute().Left(0).Top(0).Size(1, 1).DrawOver(func(*ui.Painter, ui.Rect) {
				if !advanced {
					advanced = true
					phase++
					c.Invalidate()
				}
			})
		}
	}, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	w.tester.SetLocale("en")
	w.stream, enabled = &liveStreamSamples{}, true
	w.tester.Frame()
	if phase != 2 || cv.Turns[0].Reply.Status != conversation.Completed || !w.tester.HasText(final) {
		t.Fatal("the fixture did not settle to its rendered terminal reply in one Frame call")
	}
	if _, shown := w.tester.Find(first); shown {
		t.Fatal("the settled frame still contains the first active sample")
	}
	// Admission may be observed only after Send's own settling frames. The
	// sampler must already retain both painted samples before w.turn is set.
	if w.turn != (protocol.TurnID{}) || w.stream.turn != turn || w.stream.first != len(first) || w.stream.last != len(grown) {
		t.Fatalf("painted active samples were lost: turn=%v samples=%+v", w.turn, w.stream)
	}
}

func TestLiveStreamSamplesRequireVisibleActiveReplyGrowth(t *testing.T) {
	const first = "A fixture kite rises."
	const grown = first + " Its red tail follows."
	for _, fixture := range []string{"visible", "projection_only", "prompt_only", "clipped", "terminal_only", "unchanged", "wrong_turn", "wrong_input"} {
		t.Run(fixture, func(t *testing.T) {
			session, turn := protocol.SessionID{2}, protocol.TurnID{3}
			input := "Describe the fixture kite"
			if fixture == "prompt_only" {
				input = "Repeat this example: " + grown
			}
			cv := &viewmodel.Conversation{ID: session.String()}
			w := &liveWindow{window: &window{app: shell.NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})},
				session: protocol.Session{ID: session}, input: input}
			w.app.Sidebar.Selected = cv.ID
			if fixture == "wrong_turn" {
				w.turn = protocol.TurnID{8}
			}
			text := first
			w.tester = ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				user, status := input, conversation.Running
				if fixture == "wrong_input" {
					user = "Another input"
				}
				if fixture == "terminal_only" {
					status = conversation.Completed
				}
				cv.Turns = []conversation.Turn{{ID: turn.String(), User: user, Reply: conversation.Reply{Status: status,
					Blocks: []conversation.Block{{Kind: conversation.TextBlock, Text: text}}}}}
				ui.Column(c).Fill().Role(ui.RoleLog).Children(func() {
					ui.Text(c, input)
					rendered := text
					if fixture == "projection_only" {
						rendered = first
					}
					if fixture == "clipped" {
						ui.Column(c).Height(1).Clip().Children(func() { kit.Markdown(c, rendered, kit.MarkdownProps{}) })
					} else if fixture != "prompt_only" {
						kit.Markdown(c, rendered, kit.MarkdownProps{})
					}
				})
				w.observeStreamFrame(c)
			}, 600, 400)
			w.stream = &liveStreamSamples{}
			w.tester.Frame()
			if fixture != "unchanged" {
				text = grown
			}
			w.tester.Frame()
			grew := w.stream.first > 0 && w.stream.last > w.stream.first
			if grew != (fixture == "visible") {
				t.Fatalf("%s incorrectly classified visible active growth: %+v", fixture, w.stream)
			}
		})
	}
}
