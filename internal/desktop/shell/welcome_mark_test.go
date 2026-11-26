package shell

import (
	"crypto/sha256"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Custom welcome lettering replaces the default lettering beside the brand,
// fits a narrow window by wrapping, and a blank one shows the default again.
func TestWelcomeMark(t *testing.T) {
	for _, tc := range []struct {
		name, mark string
		width      int
		wrapped    bool
	}{
		{"default", "", 1280, false},
		{"short", "Hello, World!", 1280, false},
		{"mixed", "Ship v2.0 @ 9am: OK? [yes] {no} <go> 100% & $5 #1 ~_~", 1280, true},
		{"narrow", "Hello, World! Agents at work", 560, true},
		{"tiny", "Supercalifragilistic", 360, true},
		{"tiny-long", "Hello, World! Agents at work", 360, true},
	} {
		for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
			name := fmt.Sprintf("welcome-mark-%s-%d", tc.name, mode)
			a, tt := newTester(t)
			a.Mode = mode
			newChat(t, a, tt)
			a.NavOpen = tc.width >= 1280
			tt.SetSize(tc.width, 820)
			// A line's height at this width.
			a.Messages.Mark = "Hi"
			tt.Advance(time.Second)
			tt.Frame()
			line, ok := tt.Find("Hi")
			if !ok {
				t.Fatalf("%s: no lettering in %q", name, tt.Texts())
			}
			a.Messages.Mark = tc.mark
			tt.Advance(time.Second)
			tt.Frame()
			label := tc.mark
			if label == "" {
				label = conversation.DefaultWordmark
			}
			r, ok := tt.Find(label)
			if !ok {
				t.Fatalf("%s: no lettering labeled %q in %q", name, label, tt.Texts())
			}
			if r.X < 0 || r.X+r.W > float32(tc.width) || r.W <= 0 {
				t.Errorf("%s: lettering at %+v outside the window", name, r)
			}
			if wrapped := r.H > line.H*1.5; tc.mark != "" && wrapped != tc.wrapped {
				t.Errorf("%s: lettering %v x %v, wrapped %v", name, r.W, r.H, wrapped)
			}
			snapshot(t, tt, name)
			before := sha256.Sum256(tt.Image().Pix)
			tt.Move(r.X+r.W*0.15, r.Y+r.H/2)
			tt.Frame()
			snapshot(t, tt, name+"-lit")
			if sha256.Sum256(tt.Image().Pix) == before {
				t.Errorf("%s: the light does not follow the pointer", name)
			}
		}
	}
}

func TestWelcomeDefaultMark(t *testing.T) {
	for _, tc := range []struct {
		name, defaultMark string
		width             int
		wrapped           bool
	}{
		{"harness-wide", "SAILRY HARNESS", 1280, false},
		{"code-wide", "SAILRY CODE", 1280, false},
		{"harness-narrow", "SAILRY HARNESS", 360, true},
		{"code-narrow", "SAILRY CODE", 360, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			a, tt := newTester(t)
			a.Messages.DefaultMark = tc.defaultMark
			newChat(t, a, tt)
			a.NavOpen = tc.width >= 1280
			tt.SetSize(tc.width, 820)
			a.Messages.Mark = "Hi"
			tt.Advance(time.Second)
			tt.Frame()
			line, ok := tt.Find("Hi")
			if !ok {
				t.Fatalf("no baseline lettering in %q", tt.Texts())
			}

			// Clearing user Mark reveals DefaultMark.
			a.Messages.Mark = ""
			tt.Advance(time.Second)
			tt.Frame()
			r, ok := tt.Find(tc.defaultMark)
			if !ok {
				t.Fatalf("no default lettering %q in %q", tc.defaultMark, tt.Texts())
			}
			if r.X < 0 || r.X+r.W > float32(tc.width) || r.W <= 0 {
				t.Errorf("default lettering %+v outside width %d", r, tc.width)
			}
			if wrapped := r.H > line.H*1.5; wrapped != tc.wrapped {
				t.Errorf("default lettering %v x %v, wrapped %v, want %v", r.W, r.H, wrapped, tc.wrapped)
			}
		})
	}
}
