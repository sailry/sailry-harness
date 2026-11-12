package conversation_test

import (
	"image"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// pair shows two conversations side by side on one State, as a split or a
// panel transcript beside the conversation does: "a" on the left, "b" in a
// column right wide on the right, each with its navigation.
func pair(s *conversation.State, a, b []conversation.Turn, ra, rb conversation.Actions, right float32) *ui.Tester {
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Row(c).Fill().Children(func() {
			ui.Column(c.Key("a")).Grow(1).FillHeight().Children(func() {
				conversation.View(c, s, "a", a, &conversation.Session{}, ra)
				conversation.Navigation(c, s)
			})
			ui.Column(c.Key("b")).Width(right).FillHeight().Children(func() {
				conversation.View(c, s, "b", b, &conversation.Session{}, rb)
				conversation.Navigation(c, s)
			})
		})
	}, 1400, 700)
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	return tt
}

func answered(id, user, answer string) conversation.Turn {
	return conversation.Turn{ID: id, User: user, Reply: conversation.Reply{Status: conversation.Completed, Blocks: []conversation.Block{
		{Kind: conversation.TextBlock, Text: answer},
	}}}
}

// A drag selecting text in one transcript is not ended by the other
// transcript on screen: the pill waits for the pointer to let go.
func TestSelectionBesideAnotherTranscript(t *testing.T) {
	s := conversation.New()
	tt := pair(s, []conversation.Turn{answered("a1", "Left asks", "Left answers in one line")},
		[]conversation.Turn{answered("b1", "Right asks", "Right answers")}, &recorder{}, &recorder{}, 700)
	at, ok := tt.Find("Left answers in one line")
	if !ok {
		t.Fatalf("no reply: %q", tt.Texts())
	}
	quote := func() bool {
		_, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "message_quote"))
		return ok
	}
	tt.Press(at.X+1, at.Y+at.H/2)
	tt.Move(at.X+at.W/2, at.Y+at.H/2)
	tt.Frame()
	tt.Move(at.X+at.W-1, at.Y+at.H/2)
	tt.Frame()
	tt.Frame()
	if quote() {
		t.Fatal("the pill shows while the button is held")
	}
	tt.Release(at.X+at.W-1, at.Y+at.H/2)
	tt.Frame()
	tt.Frame()
	if !quote() {
		t.Fatalf("no pill once the pointer let go: %q", tt.Texts())
	}
}

// Each transcript has its own navigation rail: a narrow one built later
// does not take the wide one's turns or hide it.
func TestNavigationBesideAnotherTranscript(t *testing.T) {
	s := conversation.New()
	a := []conversation.Turn{answered("a1", "One", "1"), answered("a2", "Two", "2"), answered("a3", "Three", "3")}
	b := []conversation.Turn{answered("b1", "One", "1"), answered("b2", "Two", "2"), answered("b3", "Three", "3"),
		answered("b4", "Four", "4"), answered("b5", "Five", "5")}
	tt := pair(s, a, b, &recorder{}, &recorder{}, 300)
	tt.Frame()
	r, value := rail(t, tt)
	if r.X != 0 || r.W != 50 || value != "3/3" {
		t.Fatalf("rail %v reads %q, want the left transcript's at 3/3", r, value)
	}
}

// The rewind confirmation and the image viewer belong to the transcript
// that opened them: they are built once and act on that conversation.
func TestDialogsOfOneTranscript(t *testing.T) {
	pic := ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 40, 30)))
	a := []conversation.Turn{{ID: "a1", User: "Look", Images: []conversation.Image{{ID: "p", Name: "left.png", Width: 40, Height: 30, Source: pic}}}}
	b := []conversation.Turn{answered("b1", "Right asks", "Right answers")}

	t.Run("rewind", func(t *testing.T) {
		s, ra, rb := conversation.New(), &recorder{}, &recorder{}
		tt := pair(s, a, b, ra, rb, 700)
		s.AskRewind("a", "a1")
		tt.Frame()
		tt.Advance(1e9)
		if n := count(tt.Texts(), i18n.In("en", "chat_rewind_confirm")); n != 1 {
			t.Fatalf("%d rewind confirmations", n)
		}
		if err := tt.Click(i18n.In("en", "chat_rewind_action")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if len(ra.rewinds) != 1 || ra.rewinds[0] != "a1" || len(rb.rewinds) != 0 {
			t.Fatalf("rewinds went to a %v and b %v", ra.rewinds, rb.rewinds)
		}
	})

	t.Run("viewer", func(t *testing.T) {
		s, ra, rb := conversation.New(), &images{}, &images{}
		tt := pair(s, a, b, ra, rb, 700)
		if err := tt.Click("left.png"); err != nil {
			t.Fatal(err)
		}
		tt.Advance(1e9)
		if err := tt.Click("Copy"); err != nil {
			t.Fatal(err, tt.Texts())
		}
		if len(ra.got) != 1 || ra.got[0] != kit.ImageCopy || len(rb.got) != 0 {
			t.Fatalf("Copy went to a %v and b %v", ra.got, rb.got)
		}
	})
}

func count(texts []string, text string) int {
	n := 0
	for _, s := range texts {
		if s == text {
			n++
		}
	}
	return n
}
