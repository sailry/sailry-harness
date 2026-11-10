package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/speech"
	"github.com/sailry/sailry-harness/internal/speech/capture"
	"github.com/sailry/sailry-harness/internal/speech/models"
)

// dictationFake is the microphone and the recognizer of a test: the frames
// the test feeds, and decodes the test may hold.
type dictationFake struct {
	mu        sync.Mutex
	granted   bool
	permit    chan struct{} // closed to let the permission answer
	stubborn  bool          // the permission ignores a cancellation
	opening   error
	loading   error
	pending   int
	amplitude float32
	broken    bool
	stopped   int
	closed    int
	released  int
	decodes   [][]float32
	decode    func(audio []float32) (string, error)
}

func (f *dictationFake) feed(frames int) {
	f.mu.Lock()
	f.pending += frames
	f.mu.Unlock()
}

func (f *dictationFake) count(read func() int) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return read()
}

func (f *dictationFake) backend() dictationBackend {
	return dictationBackend{
		permission: func(ctx context.Context) (bool, error) {
			f.mu.Lock()
			permit, stubborn, granted := f.permit, f.stubborn, f.granted
			f.mu.Unlock()
			if permit != nil {
				if stubborn {
					<-permit
				} else {
					select {
					case <-permit:
					case <-ctx.Done():
						return false, ctx.Err()
					}
				}
			}
			return granted, nil
		},
		recognizer: func(string, string, speech.Language) (dictationRecognizer, error) {
			f.mu.Lock()
			defer f.mu.Unlock()
			if f.loading != nil {
				return dictationRecognizer{}, f.loading
			}
			return dictationRecognizer{
				recognize: func(audio []float32) (string, error) {
					f.mu.Lock()
					f.decodes = append(f.decodes, slices.Clone(audio))
					decode := f.decode
					f.mu.Unlock()
					return decode(audio)
				},
				close: func() { f.mu.Lock(); f.released++; f.mu.Unlock() },
			}, nil
		},
		open: func(*string) (dictationSource, error) {
			f.mu.Lock()
			defer f.mu.Unlock()
			if f.opening != nil {
				return dictationSource{}, f.opening
			}
			return dictationSource{
				rate: 32_000, channels: 2,
				read: func(buffer []float32) int {
					f.mu.Lock()
					defer f.mu.Unlock()
					frames := min(f.pending, len(buffer)/2)
					for index := range frames * 2 {
						buffer[index] = f.amplitude
					}
					f.pending -= frames
					return frames
				},
				failed: func() bool { f.mu.Lock(); defer f.mu.Unlock(); return f.broken },
				stop:   func() { f.mu.Lock(); f.stopped++; f.mu.Unlock() },
				close:  func() { f.mu.Lock(); f.closed++; f.mu.Unlock() },
			}, nil
		},
	}
}

// checkDictation walks the composer's voice input through preparing,
// recording, transcribing and the insertion, cancels it in each state and
// fails it at each step, with a fake microphone and recognizer.
func checkDictation(t *testing.T) {
	node := protocol.NodeID{7}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	controller := client.New(&conversationTestTransport{snapshot: snapshot})
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	const draft = "draft/dictation"
	cv := &viewmodel.Conversation{ID: draft, Node: node.String()}
	data := &viewmodel.Data{
		Workspace: &viewmodel.Workspace{Hosts: []viewmodel.Host{{ID: node.String(), Local: true}}},
		Projects:  []*viewmodel.Project{{ID: "project", Host: node.String(), Conversations: []*viewmodel.Conversation{cv}}},
	}

	// An installed registered model stands for the downloaded one: the
	// recommended model's pinned files are too large for a test.
	directory := filepath.Join(t.TempDir(), "desktop")
	id := protocol.NewModelID()
	source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: "org/voice", Revision: "0123456789abcdef0123456789abcdef01234567"}
	installed := models.Record{ID: id, Installed: true, Definition: protocol.ModelsDefinition{
		Source: source, Name: "Voice", License: "mit", Format: protocol.ModelsFormat{Kind: "sense_voice", Data: &protocol.ModelsFormatSenseVoicePayload{}}}}
	if err := os.MkdirAll(filepath.Join(directory, "models", "speech", id.String()), 0o700); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"model.onnx", "tokens.txt"} {
		content := []byte("fixture " + name)
		sum := sha256.Sum256(content)
		installed.Definition.Assets = append(installed.Definition.Assets, protocol.ModelsAsset{Source: source, Path: name, Name: name, Bytes: uint64(len(content)), Sha256: hex.EncodeToString(sum[:])})
		if err := os.WriteFile(filepath.Join(directory, "models", "speech", id.String(), name), content, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	index, err := json.Marshal([]models.Record{installed})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, "models", "speech.json"), index, 0o600); err != nil {
		t.Fatal(err)
	}

	ctx, cancel := context.WithCancel(t.Context())
	r := &Runtime{ctx: ctx, cancel: cancel, controller: controller, App: shell.NewLive(data),
		Preferences: preferences.Open(filepath.Join(directory, "preferences.json"))}
	defer func() { cancel(); r.workers.Wait() }()
	r.Preferences.Data.Dictation = &preferences.Dictation{Language: "english", Model: &id}
	fake := &dictationFake{granted: true, amplitude: 0.1}
	r.dictation.backend = fake.backend()
	state := &r.dictation

	var (
		c      *ui.Context
		toasts []string
		click  func() // a click, which runs inside a frame as in the window
	)
	tester := ui.NewTester(func(view *ui.Context) {
		c = view
		if click != nil {
			click()
			click = nil
		}
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, post := range queue {
			post(view)
		}
		ui.ToastViewportBase(view, func(_ ui.Element, shown []ui.Toast) {
			toasts = toasts[:0]
			for _, toast := range shown {
				toasts = append(toasts, toast.Title)
			}
		})
	}, 640, 400)
	tester.SetLocale("en")
	tester.Frame()
	await := func(what string, done func() bool) {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for tester.Frame(); !done(); tester.Frame() {
			if time.Now().After(deadline) {
				t.Fatalf("timed out waiting for %s", what)
			}
			time.Sleep(2 * time.Millisecond)
		}
	}
	told := func(key string) bool { tester.Frame(); return slices.Contains(toasts, i18n.In("en", key)) }
	comp := composer.New("model")
	d := &comp.Dictation
	dictate := func(command composer.DictationCommand) {
		click = func() { (&conversationActions{r: r, c: c, cv: cv, comp: comp}).Dictate(d, command) }
		tester.Frame()
	}
	idle := func() bool { return state.active == nil && d.Phase == composer.Idle }
	start := func() {
		t.Helper()
		dictate(composer.StartDictation)
		if d.Phase != composer.PreparingSpeech || state.active == nil {
			t.Fatalf("started dictation is in phase %d", d.Phase)
		}
	}
	record := func() {
		t.Helper()
		start()
		await("recording", func() bool { return d.Phase == composer.Recording })
	}
	reset := func() {
		fake.mu.Lock()
		fake.granted, fake.permit, fake.stubborn, fake.opening, fake.loading = true, nil, false, nil, nil
		fake.pending, fake.amplitude, fake.broken, fake.decodes = 0, 0.1, false, nil
		fake.stopped, fake.closed, fake.released = 0, 0, 0
		fake.decode = func([]float32) (string, error) { return "", nil }
		fake.mu.Unlock()
		comp.Draft = ""
		comp.ReplaceDictation(0, 0, "")
	}

	// Prepare, record, transcribe, insert. The transcript goes to the
	// caret; each later hypothesis replaces the one before it.
	reset()
	comp.Draft = "Say  now"
	comp.ReplaceDictation(4, 4, "")
	permit := make(chan struct{})
	fake.permit = permit
	fake.decode = func(audio []float32) (string, error) { return "one", nil }
	start()
	dictate(composer.StartDictation)
	if d.Phase != composer.PreparingSpeech || fake.count(func() int { return fake.closed }) != 0 {
		t.Fatal("a second click while preparing started again")
	}
	close(permit)
	await("recording", func() bool { return d.Phase == composer.Recording })
	fake.feed(32_000)
	await("levels", func() bool { return len(d.Levels) > 0 })
	if level := d.Levels[len(d.Levels)-1]; level < 0.6 || level > 0.7 {
		t.Fatalf("level of a 0.1 signal = %v", level)
	}
	await("partial transcript", func() bool { return comp.Draft == "Say one now" })
	final := make(chan struct{})
	fake.mu.Lock()
	fake.decode = func([]float32) (string, error) { <-final; return "one two three", nil }
	fake.mu.Unlock()
	fake.feed(16_000)
	await("the fed audio", func() bool { return fake.count(func() int { return fake.pending }) == 0 })
	dictate(composer.FinishDictation)
	if d.Phase != composer.Transcribing || d.Levels != nil {
		t.Fatalf("finished recording is in phase %d", d.Phase)
	}
	await("final decode", func() bool { return fake.count(func() int { return len(fake.decodes) }) == 2 })
	if d.Phase != composer.Transcribing || comp.Draft != "Say one now" {
		t.Fatalf("while transcribing: phase %d, draft %q", d.Phase, comp.Draft)
	}
	close(final)
	await("idle", idle)
	if comp.Draft != "Say one two three now" || comp.DictationCaret() != 17 {
		t.Fatalf("draft = %q, caret %d", comp.Draft, comp.DictationCaret())
	}
	// 48 000 stereo frames at 32 kHz are 1.5 s of mono audio at 16 kHz.
	if last := fake.decodes[1]; len(last) != 24_000 || last[0] != 0.1 || last[len(last)-1] != 0.1 {
		t.Fatalf("final decode saw %d samples", len(last))
	}
	if fake.stopped != 1 || fake.closed != 1 || fake.released != 1 || len(toasts) != 0 {
		t.Fatalf("stopped %d, closed %d, recognizer closed %d, toasts %v", fake.stopped, fake.closed, fake.released, toasts)
	}

	// Cancel while preparing. The worker still holds the model until it
	// returns, and another dictation is refused meanwhile.
	reset()
	permit = make(chan struct{})
	fake.permit, fake.stubborn = permit, true
	start()
	dictate(composer.CancelDictation)
	if d.Phase != composer.Idle || state.active == nil {
		t.Fatal("cancelling while preparing left the button busy or released the owner early")
	}
	dictate(composer.StartDictation)
	if d.Phase != composer.Idle || !told("dictation_busy") {
		t.Fatalf("a dictation started over a running worker: %v", toasts)
	}
	if err := state.manager.Remove(id); !errors.Is(err, models.ErrBusy) {
		t.Fatalf("removing the model of a running worker: %v", err)
	}
	close(permit)
	await("idle", idle)
	if comp.Draft != "" || fake.closed != 0 {
		t.Fatal("a cancelled preparation recorded")
	}

	// Cancel while recording.
	reset()
	fake.decode = func([]float32) (string, error) { return "never", nil }
	record()
	fake.feed(3_200)
	await("levels", func() bool { return len(d.Levels) > 0 })
	dictate(composer.CancelDictation)
	if d.Phase != composer.Idle || d.Levels != nil {
		t.Fatal("cancelling a recording left the meter")
	}
	await("idle", idle)
	if comp.Draft != "" || fake.closed != 1 || fake.released != 1 || len(fake.decodes) != 0 {
		t.Fatalf("cancelled recording: draft %q, closed %d, decodes %d", comp.Draft, fake.closed, len(fake.decodes))
	}

	// Cancel while transcribing: the late transcript is dropped.
	reset()
	final = make(chan struct{})
	fake.decode = func([]float32) (string, error) { <-final; return "late", nil }
	record()
	fake.feed(6_400)
	await("the fed audio", func() bool { return fake.count(func() int { return fake.pending }) == 0 })
	dictate(composer.FinishDictation)
	await("final decode", func() bool { return fake.count(func() int { return len(fake.decodes) }) == 1 })
	dictate(composer.CancelDictation)
	if d.Phase != composer.Idle || state.active == nil {
		t.Fatal("cancelling a transcription")
	}
	close(final)
	await("idle", idle)
	if comp.Draft != "" {
		t.Fatalf("a cancelled transcript was inserted: %q", comp.Draft)
	}

	// A draft edited under the partial transcript stops the dictation.
	reset()
	fake.decode = func([]float32) (string, error) { return "partial", nil }
	record()
	fake.feed(32_000)
	await("partial transcript", func() bool { return comp.Draft == "partial" })
	comp.Draft = "parted"
	fake.feed(32_000)
	await("idle", idle)
	if comp.Draft != "parted" || !told("dictation_draft_changed") {
		t.Fatalf("edited draft = %q, %v", comp.Draft, toasts)
	}

	// Each failure is its own message and leaves the draft alone.
	for _, failure := range []struct {
		key     string
		arrange func()
		feed    int
	}{
		{"dictation_permission", func() { fake.granted = false }, 0},
		{"dictation_unavailable", func() { fake.loading = speech.ErrUnavailable }, 0},
		{"dictation_model_invalid", func() { fake.loading = speech.ErrInvalidModel }, 0},
		{"dictation_microphone", func() { fake.opening = errors.New("no device") }, 0},
		{"dictation_no_audio", nil, 0},
		{"dictation_silent", func() { fake.amplitude = 0 }, 6_400},
		{"dictation_empty", nil, 6_400},
		{"dictation_failed", func() { fake.decode = func([]float32) (string, error) { return "", speech.ErrRecognition } }, 6_400},
	} {
		reset()
		comp.Draft = "kept"
		if failure.arrange != nil {
			fake.mu.Lock()
			failure.arrange()
			fake.mu.Unlock()
		}
		start()
		await(failure.key, func() bool {
			if d.Phase == composer.Recording {
				if failure.feed > 0 {
					fake.feed(failure.feed)
					failure.feed = 0
				}
				if fake.count(func() int { return fake.pending }) == 0 {
					dictate(composer.FinishDictation)
				}
			}
			return idle()
		})
		if comp.Draft != "kept" || !told(failure.key) {
			t.Fatalf("%s: draft %q, toasts %v", failure.key, comp.Draft, toasts)
		}
	}
	reset()
	record()
	fake.mu.Lock()
	fake.broken = true
	fake.mu.Unlock()
	await("idle", idle)
	// The window shows its latest toasts; the last one before was the
	// failed transcription's.
	if tester.Frame(); toasts[len(toasts)-1] != i18n.In("en", "dictation_microphone") || fake.closed != 1 {
		t.Fatalf("a microphone that stopped: %v", toasts)
	}

	// A conversation that cannot be written to does not dictate.
	reset()
	click = func() {
		foreign := &viewmodel.Conversation{ID: draft, Node: (protocol.NodeID{9}).String()}
		(&conversationActions{r: r, c: c, cv: foreign, comp: comp}).Dictate(d, composer.StartDictation)
	}
	tester.Frame()
	if d.Phase != composer.Idle || state.active != nil {
		t.Fatal("a read-only conversation started a dictation")
	}

	// The settings rows: the recommended model first, then the registered
	// ones; only an installed model is selected.
	rows, chosen := speechModels(&id, state.manager)
	if len(rows) != 2 || rows[0].ID != "" || rows[0].Status != preview.ModelMissing || chosen != 1 ||
		rows[1].ID != id.String() || rows[1].Name != "Voice" || rows[1].Repo != "org/voice" || rows[1].Status != preview.ModelInstalled {
		t.Fatalf("speech rows = %+v, selected %d", rows, chosen)
	}
	if status, percent := speechStatus(models.State{Status: models.Downloading}); status != preview.ModelDownloading || percent != 0 {
		t.Fatalf("downloading row = %v, %v", status, percent)
	}
	if status, _ := speechStatus(models.State{Status: models.Failed, Err: errors.New("failed")}); status != preview.ModelMissing {
		t.Fatalf("failed row = %v", status)
	}
	r.selectSpeechModel("")
	if r.Preferences.Data.DictationModel() == nil || !told("dictation_model_missing") {
		t.Fatal("a model that is not installed was selected")
	}

	// A missing model opens Settings › Dictation and downloads nothing.
	before := slices.Clone(toasts)
	r.Preferences.Data.Dictation.Model = nil
	start()
	await("idle", idle)
	if r.App.Page != shell.SettingsPage || r.App.Settings.Section != settings.Dictation || !slices.Equal(toasts, before) {
		t.Fatalf("missing model: page %v, toasts %v", r.App.Page, toasts)
	}
	if _, err := os.Stat(filepath.Join(directory, "models", "sensevoice-int8")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("a dictation created the recommended model's directory: %v", err)
	}
	unknown := protocol.NewModelID()
	r.Preferences.Data.Dictation.Model = &unknown
	r.App.Navigate(shell.ConversationPage)
	dictate(composer.StartDictation)
	if d.Phase != composer.Idle || state.active != nil || r.App.Page != shell.SettingsPage {
		t.Fatal("an unregistered model started a dictation")
	}

	// The microphone picker: the system default, the devices, and a
	// stored device that is gone as an entry of its own.
	devices := []capture.Device{{ID: "a1", Name: "Built-in", Default: true}, {ID: "b2", Name: "USB"}}
	stored, gone := "b2", "c3"
	if names, at := microphoneChoices(devices, nil, "absent"); len(names) != 2 || at != 0 {
		t.Fatalf("default microphone = %v at %d", names, at)
	}
	if names, at := microphoneChoices(devices, &stored, "absent"); len(names) != 2 || names[1] != "USB" || at != 2 {
		t.Fatalf("stored microphone = %v at %d", names, at)
	}
	if names, at := microphoneChoices(devices, &gone, "absent"); len(names) != 3 || names[2] != "absent" || at != 3 {
		t.Fatalf("absent microphone = %v at %d", names, at)
	}
	for err, key := range map[error]string{
		capture.ErrUnavailable: "dictation_unavailable", models.ErrBusy: "dictation_busy", models.ErrStorage: "dictation_storage",
		models.ErrMissing: "dictation_model_missing", errors.New("other"): "dictation_failed",
	} {
		if got := dictationKey(err); got != key {
			t.Fatalf("%v is reported as %s", err, got)
		}
	}
}
