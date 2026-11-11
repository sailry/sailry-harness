package reader

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/audio"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestReadAloud reads with a fake model voice, player and OS speech: the
// model reads what it can, in parts and in order; the OS speech reads what
// it cannot; Stop ends both.
func TestReadAloud(t *testing.T) {
	type fixture struct {
		mu          sync.Mutex // the UI thread
		service     *Service
		synthesized []string
		played      []string
		spoken      []string
		speaking    bool
		failed      int
	}
	// start makes a service whose model voice answers with synthesize and
	// whose player plays with play; nil is a voice and a player that work.
	start := func(synthesize func(ctx context.Context, index int, text string) error, play func(ctx context.Context) error, speak error) *fixture {
		f := &fixture{}
		f.service = New(func(run func()) {
			f.mu.Lock()
			defer f.mu.Unlock()
			run()
		}, nil)
		f.service.SetSynthesizer(func(ctx context.Context, text string, voice *string) (string, []byte, error) {
			f.mu.Lock()
			index := len(f.synthesized)
			f.synthesized = append(f.synthesized, text)
			f.mu.Unlock()
			if voice != nil {
				t.Errorf("voice %q, want the Node's default", *voice)
			}
			if synthesize != nil {
				if err := synthesize(ctx, index, text); err != nil {
					return "", nil, err
				}
			}
			return "audio/mpeg", []byte(text), nil
		})
		f.service.play = func(ctx context.Context, clip []byte) error {
			f.mu.Lock()
			f.played = append(f.played, string(clip))
			f.mu.Unlock()
			if play != nil {
				return play(ctx)
			}
			return nil
		}
		f.service.open = func() (*voice, error) {
			return &voice{
				speak: func(text string) error {
					if speak != nil {
						return speak
					}
					f.spoken, f.speaking = append(f.spoken, text), true
					return nil
				},
				stop:     func() error { f.speaking = false; return nil },
				speaking: func() (bool, error) { return false, nil },
				close:    func() error { return nil },
			}, nil
		}
		return f
	}
	toggle := func(f *fixture, markdown string) {
		t.Helper()
		f.mu.Lock()
		defer f.mu.Unlock()
		if err := f.service.Toggle("turn", markdown, func() { f.failed++ }); err != nil {
			t.Fatal(err)
		}
	}
	// settled waits for the reading to end and returns what was heard.
	settled := func(f *fixture) (synthesized, played, spoken []string, failed int) {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for {
			f.mu.Lock()
			active := f.service.Active("turn")
			synthesized, played, spoken, failed = f.synthesized, f.played, f.spoken, f.failed
			f.mu.Unlock()
			if !active {
				return
			}
			if time.Now().After(deadline) {
				t.Fatal("the reading did not end")
			}
			time.Sleep(2 * time.Millisecond)
		}
	}
	var long strings.Builder
	for i := range 900 {
		fmt.Fprintf(&long, "Sentence number %d ends here. ", i)
	}
	text := strings.TrimSpace(long.String())
	parts := Chunks(text, maxSpeechCharacters)
	missing := &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeNotConfigured, Message: "no speech model"}}

	t.Run("parts", func(t *testing.T) {
		if len(parts) < 3 || strings.Join(parts, " ") != text {
			t.Fatalf("%d parts do not rejoin to the text", len(parts))
		}
		for _, part := range parts {
			if n := utf8.RuneCountInString(part); n < 1 || n > maxSpeechCharacters || !strings.HasSuffix(part, ".") {
				t.Fatalf("part of %d characters ending %q", n, part[len(part)-10:])
			}
		}
		for _, c := range []struct {
			text  string
			limit int
			want  []string
		}{
			{"short", 10, []string{"short"}},
			{"", 10, nil},
			{"One. Two! Three? Four", 11, []string{"One. Two!", "Three? Four"}},
			{"第一句。第二句！第三句？", 5, []string{"第一句。", "第二句！", "第三句？"}},
			{"version 1.2 is out now", 12, []string{"version 1.2", "is out now"}},
			{"abcdefghij", 4, []string{"abcd", "efgh", "ij"}},
		} {
			if got := Chunks(c.text, c.limit); strings.Join(got, "|") != strings.Join(c.want, "|") {
				t.Errorf("Chunks(%q, %d) = %q, want %q", c.text, c.limit, got, c.want)
			}
		}
	})

	t.Run("an assigned speech model reads every part in order", func(t *testing.T) {
		f := start(nil, nil, nil)
		toggle(f, text)
		synthesized, played, spoken, failed := settled(f)
		if strings.Join(synthesized, "|") != strings.Join(parts, "|") || strings.Join(played, "|") != strings.Join(parts, "|") {
			t.Errorf("synthesized %d parts and played %d, want %d in order", len(synthesized), len(played), len(parts))
		}
		if len(spoken) != 0 || failed != 0 {
			t.Errorf("the system spoke %d texts, %d failures", len(spoken), failed)
		}
	})

	t.Run("no speech model falls back to the system", func(t *testing.T) {
		f := start(func(context.Context, int, string) error { return missing }, nil, nil)
		toggle(f, "Read **this** aloud.")
		synthesized, played, spoken, failed := settled(f)
		if len(synthesized) != 1 || len(played) != 0 || len(spoken) != 1 || spoken[0] != "Read this aloud." || failed != 0 {
			t.Errorf("synthesized %q, played %q, spoken %q, %d failures", synthesized, played, spoken, failed)
		}
	})

	t.Run("a failed part and the rest fall back to the system", func(t *testing.T) {
		f := start(func(_ context.Context, index int, _ string) error {
			if index == 1 {
				return errors.New("provider failed")
			}
			return nil
		}, nil, nil)
		toggle(f, text)
		_, played, spoken, failed := settled(f)
		if len(played) != 1 || played[0] != parts[0] || len(spoken) != 1 || spoken[0] != strings.Join(parts[1:], " ") || failed != 0 {
			t.Errorf("played %d parts, the system spoke %d texts, %d failures", len(played), len(spoken), failed)
		}
	})

	t.Run("a clip that cannot play falls back to the system", func(t *testing.T) {
		f := start(nil, func(context.Context) error { return errors.New("no audio output") }, nil)
		toggle(f, "Read this aloud.")
		if _, _, spoken, failed := settled(f); len(spoken) != 1 || spoken[0] != "Read this aloud." || failed != 0 {
			t.Errorf("the system spoke %q, %d failures", spoken, failed)
		}
	})

	t.Run("a clip that stops decoding falls back to the system", func(t *testing.T) {
		// Sixteen silent MPEG-1 layer III frames, which the player decodes
		// in nine whole reads, then one whose header is layer II's: the
		// file opens and the tenth read fails.
		silent := append([]byte{0xff, 0xfb, 0x90, 0x00}, make([]byte, 413)...)
		clip := slices.Concat(slices.Repeat(silent, 16), []byte{0xff, 0xfd, 0x90, 0x00}, make([]byte, 413))
		if err := playClip(t.Context(), clip); err == nil || err == audio.Err() {
			t.Fatalf("a clip that stops decoding played as %v", err)
		}
		f := start(nil, nil, nil)
		f.service.SetSynthesizer(func(context.Context, string, *string) (string, []byte, error) { return "audio/mpeg", clip, nil })
		f.service.play = playClip
		toggle(f, "Read this aloud.")
		if _, _, spoken, failed := settled(f); len(spoken) != 1 || spoken[0] != "Read this aloud." || failed != 0 {
			t.Errorf("the system spoke %q, %d failures", spoken, failed)
		}
	})

	t.Run("system speech failing after the model reports once", func(t *testing.T) {
		f := start(func(context.Context, int, string) error { return missing }, nil, errors.New("no voices"))
		toggle(f, "Read this aloud.")
		if _, _, spoken, failed := settled(f); len(spoken) != 0 || failed != 1 {
			t.Errorf("the system spoke %q, %d failures", spoken, failed)
		}
	})

	t.Run("stop cancels the synthesis under way and the clip playing", func(t *testing.T) {
		playing, stopped := make(chan struct{}), make(chan error, 2)
		f := start(func(ctx context.Context, index int, _ string) error {
			if index == 0 {
				return nil
			}
			// The next part is asked for while the first plays.
			<-ctx.Done()
			stopped <- ctx.Err()
			return ctx.Err()
		}, func(ctx context.Context) error {
			close(playing)
			<-ctx.Done()
			stopped <- ctx.Err()
			return ctx.Err()
		}, nil)
		toggle(f, text)
		<-playing
		f.mu.Lock()
		active := f.service.Active("turn")
		f.mu.Unlock()
		if !active {
			t.Fatal("the turn is not being read while its clip plays")
		}
		// The same turn's button stops it.
		toggle(f, text)
		for range 2 {
			select {
			case err := <-stopped:
				if !errors.Is(err, context.Canceled) {
					t.Errorf("stopped with %v", err)
				}
			case <-time.After(10 * time.Second):
				t.Fatal("stop left the synthesis or the playback running")
			}
		}
		synthesized, played, spoken, failed := settled(f)
		if len(synthesized) != 2 || len(played) != 1 || len(spoken) != 0 || failed != 0 {
			t.Errorf("synthesized %d, played %d, the system spoke %d texts, %d failures", len(synthesized), len(played), len(spoken), failed)
		}
	})
}
