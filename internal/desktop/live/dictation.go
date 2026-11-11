package live

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/netproxy"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/speech"
	"github.com/sailry/sailry-harness/internal/speech/capture"
	"github.com/sailry/sailry-harness/internal/speech/models"
)

// Dictation is the controller's: the microphone, the speech runtime and the
// speech models are this machine's, whichever Node a conversation runs on.
// The state lives on the local runtime. One dictation runs at a time; its
// worker holds the model's lease until its last decode has returned, so a
// cancelled dictation keeps the next one waiting (dictation_busy) rather
// than sharing the recognizer.

// dictationState is the local runtime's dictation: the speech-model
// manager, the dictation that runs and what Settings › Dictation shows.
type dictationState struct {
	manager *models.Manager
	failed  bool // the speech-model index could not be read
	active  *dictationSession
	backend dictationBackend
	page    dictationPage
}

// dictationBackend is the microphone and the recognizer, which tests
// replace.
type dictationBackend struct {
	// recognizer loads the speech runtime and the model.
	recognizer func(graph, tokens string, language speech.Language) (dictationRecognizer, error)
	// permission reports microphone access, asking the system once when
	// it was never decided.
	permission func(context.Context) (bool, error)
	// open starts the microphone, the system default for no device.
	open func(device *string) (dictationSource, error)
}

type dictationRecognizer struct {
	recognize func([]float32) (string, error)
	close     func()
}

// dictationSource is a started microphone: interleaved frames at its own
// rate.
type dictationSource struct {
	rate     uint32
	channels int
	read     func([]float32) int
	failed   func() bool
	stop     func() // no frame arrives after it
	close    func()
}

// speechRuntime loads the pinned speech and capture libraries from the
// executable's directory, once.
func speechRuntime() error {
	if err := speech.Initialize(); err != nil {
		return err
	}
	directory, err := onnx.Directory()
	if err != nil {
		return err
	}
	return capture.Load(directory, core.Release())
}

func systemDictation() dictationBackend {
	return dictationBackend{
		recognizer: func(graph, tokens string, language speech.Language) (dictationRecognizer, error) {
			if err := speechRuntime(); err != nil {
				return dictationRecognizer{}, speech.ErrUnavailable
			}
			recognizer, err := speech.NewRecognizer(graph, tokens, language)
			if err != nil {
				return dictationRecognizer{}, err
			}
			return dictationRecognizer{recognize: recognizer.Recognize, close: recognizer.Close}, nil
		},
		permission: capture.Request,
		open: func(device *string) (dictationSource, error) {
			if err := speechRuntime(); err != nil {
				return dictationSource{}, err
			}
			id := ""
			if device != nil {
				id = *device
			}
			recorder, err := capture.Open(id)
			if err != nil {
				return dictationSource{}, err
			}
			if err := recorder.Start(); err != nil {
				recorder.Close()
				return dictationSource{}, err
			}
			return dictationSource{rate: recorder.SampleRate, channels: recorder.Channels, read: recorder.Read, failed: recorder.Failed,
				stop: func() { _ = recorder.Stop() }, close: recorder.Close}, nil
		},
	}
}

// validateSpeechModel constructs the recognizer over downloaded files, so
// that a model the runtime cannot load is not installed.
func validateSpeechModel(graph, tokens string) error {
	recognizer, err := systemDictation().recognizer(graph, tokens, speech.Auto)
	if err != nil {
		return err
	}
	recognizer.close()
	return nil
}

// speechManager opens the controller's speech models under the desktop
// data directory, beside the preferences. Its downloads use the desktop's
// proxy settings and end with the runtime.
func (r *Runtime) speechManager() *models.Manager {
	root := r.localRuntime()
	state := &root.dictation
	if state.manager != nil || state.failed || root.Preferences == nil {
		return state.manager
	}
	manager, err := models.Open(root.ctx, models.Options{
		Directory: filepath.Dir(root.Preferences.Path), Transport: netproxy.Transport(),
		Validate: validateSpeechModel, Changed: root.wake,
	})
	if err != nil {
		state.failed = true
		return nil
	}
	state.manager = manager
	return manager
}

// dictationKey is the message of a failure, as the Rust desktop names it.
func dictationKey(err error) string {
	switch {
	case errors.Is(err, speech.ErrUnavailable), errors.Is(err, capture.ErrUnavailable):
		return "dictation_unavailable"
	case errors.Is(err, models.ErrMissing):
		return "dictation_model_missing"
	case errors.Is(err, models.ErrBusy):
		return "dictation_busy"
	case errors.Is(err, models.ErrStorage), errors.Is(err, assets.ErrStorage):
		return "dictation_storage"
	case errors.Is(err, assets.ErrDownload):
		return "dictation_download"
	case errors.Is(err, speech.ErrInvalidModel), errors.Is(err, assets.ErrInvalidAsset):
		return "dictation_model_invalid"
	}
	return "dictation_failed"
}

// dictationNotice shows a dictation's outcome. A missing model opens
// Settings › Dictation instead; what is not a failure is a plain message.
func (r *Runtime) dictationNotice(c *ui.Context, key string) {
	root := r.localRuntime()
	switch key {
	case "dictation_model_missing":
		root.App.Settings.Open(settings.Dictation)
		root.App.Navigate(shell.SettingsPage)
	case "dictation_empty", "dictation_draft_changed":
		kit.Message(c, i18n.T(c, key))
	default:
		kit.Toast(c, kit.Failed, i18n.T(c, key))
	}
}

// Dictate moves the composer's voice input: idle it starts, recording it
// finishes and otherwise it cancels.
func (x *conversationActions) Dictate(d *composer.Dictation, command composer.DictationCommand) {
	root := x.r.localRuntime()
	state := &root.dictation
	switch command {
	case composer.StartDictation:
		if x.readonly() || d.Phase != composer.Idle {
			return
		}
		if state.active != nil {
			root.dictationNotice(x.c, "dictation_busy")
			return
		}
		manager := root.speechManager()
		if manager == nil {
			root.dictationNotice(x.c, "dictation_storage")
			return
		}
		lease, err := manager.Capture(root.Preferences.Data.DictationModel())
		if err != nil {
			root.dictationNotice(x.c, dictationKey(err))
			return
		}
		if state.backend.recognizer == nil {
			state.backend = systemDictation()
		}
		ctx, cancel := context.WithCancel(root.ctx)
		session := &dictationSession{
			comp: x.comp, view: d, lease: lease, backend: state.backend,
			language: speech.ParseLanguage(root.Preferences.Data.DictationLanguage()),
			device:   root.Preferences.Data.DictationMicrophone(),
			ctx:      ctx, cancel: cancel, finish: make(chan struct{}),
			post: root.enqueue, readonly: x.readonly, notify: root.dictationNotice,
		}
		session.released = func() {
			if state.active == session {
				state.active = nil
			}
		}
		state.active = session
		d.Phase, d.Levels = composer.PreparingSpeech, nil
		if !root.async(session.run) {
			session.end()
			cancel()
			lease.Release()
			state.active = nil
		}
	case composer.FinishDictation:
		if session := state.active; session != nil && session.view == d && d.Phase == composer.Recording {
			session.stop()
		}
	case composer.CancelDictation:
		if session := state.active; session != nil && session.view == d {
			session.abandon()
		} else {
			d.Phase, d.Levels = composer.Idle, nil
		}
	}
}

// dictationSession is one dictation of one composer. Its worker prepares
// the model, records and transcribes; everything that touches the view runs
// on the UI thread through post.
type dictationSession struct {
	comp     *composer.State
	view     *composer.Dictation
	lease    *models.Capture
	backend  dictationBackend
	language speech.Language
	device   *string

	ctx      context.Context
	cancel   context.CancelFunc
	finish   chan struct{}
	finished sync.Once

	post     func(func(*ui.Context))
	readonly func() bool
	notify   func(*ui.Context, string)
	released func() // the worker has returned

	// The UI thread's.
	ended   bool                // the view is idle again; later updates are dropped
	partial *dictationInsertion // the transcript in the draft so far
}

// dictationInsertion is the transcript as it was put in the draft, in
// runes.
type dictationInsertion struct {
	start, end int
	text       string
}

const (
	dictationPoll    = 20 * time.Millisecond
	dictationMeter   = 60 * time.Millisecond
	dictationPartial = 800 * time.Millisecond
	dictationLevels  = 8
)

// end leaves the view idle. The UI thread calls it.
func (s *dictationSession) end() {
	s.ended = true
	s.view.Phase, s.view.Levels = composer.Idle, nil
}

// stop finishes the recording; the worker transcribes what it has.
func (s *dictationSession) stop() {
	s.finished.Do(func() { close(s.finish) })
	s.view.Phase, s.view.Levels = composer.Transcribing, nil
}

// abandon cancels the dictation. The worker stops at its next step, or
// when a decode that already runs returns.
func (s *dictationSession) abandon() {
	s.cancel()
	s.end()
}

// run is the worker: the model's files, the microphone's permission, the
// recognizer, the recording and the final transcript.
func (s *dictationSession) run() {
	defer s.post(func(*ui.Context) { s.released() })
	defer s.lease.Release()
	defer s.cancel()
	text, key := s.record()
	if s.ctx.Err() != nil {
		return
	}
	s.post(func(c *ui.Context) { s.complete(c, text, key) })
}

// record returns the transcript or the key of the failure's message.
func (s *dictationSession) record() (string, string) {
	if !s.lease.Ready() {
		return "", "dictation_model_missing"
	}
	if s.ctx.Err() != nil {
		return "", ""
	}
	granted, err := s.backend.permission(s.ctx)
	if s.ctx.Err() != nil {
		return "", ""
	}
	if err != nil || !granted {
		return "", "dictation_permission"
	}
	graph, tokens := s.lease.Paths()
	recognizer, err := s.backend.recognizer(graph, tokens, s.language)
	if err != nil {
		return "", dictationKey(err)
	}
	defer recognizer.close()
	if s.ctx.Err() != nil {
		return "", ""
	}
	source, err := s.backend.open(s.device)
	if err != nil {
		if errors.Is(err, speech.ErrUnavailable) || errors.Is(err, capture.ErrUnavailable) {
			return "", "dictation_unavailable"
		}
		return "", "dictation_microphone"
	}
	closeSource := sync.OnceFunc(source.close)
	defer closeSource()
	if source.rate == 0 || source.channels <= 0 {
		return "", "dictation_microphone"
	}
	s.post(func(*ui.Context) {
		if s.ended {
			return
		}
		if s.readonly() {
			s.abandon()
			return
		}
		s.view.Phase = composer.Recording
	})

	sampler := speech.NewSampler(source.rate)
	buffer := make([]float32, int(source.rate)/10*source.channels)
	var (
		audio   []float32
		window  []float32 // the samples since the meter last moved
		decoded int
		last    = time.Now()
		metered = time.Now()
	)
	// take drains the microphone into mono audio at the recognizer's rate.
	take := func() {
		for {
			frames := source.read(buffer)
			if frames <= 0 {
				return
			}
			for frame := range frames {
				var sum float32
				for _, sample := range buffer[frame*source.channels : (frame+1)*source.channels] {
					sum += sample
				}
				sampler.Push(sum/float32(source.channels), func(value float32) {
					window = append(window, value)
					if len(audio) < speech.MaxSamples {
						audio = append(audio, value)
					}
				})
			}
			if frames*source.channels < len(buffer) {
				return
			}
		}
	}
	ticker := time.NewTicker(dictationPoll)
	defer ticker.Stop()
recording:
	for {
		select {
		case <-s.ctx.Done():
			return "", ""
		case <-s.finish:
			break recording
		case <-ticker.C:
		}
		take()
		if source.failed() {
			return "", "dictation_microphone"
		}
		if len(window) > 0 && time.Since(metered) >= dictationMeter {
			level := capture.Level(window)
			window, metered = window[:0], time.Now()
			s.post(func(*ui.Context) { s.meter(level) })
		}
		if len(audio) >= speech.MaxSamples {
			// Five minutes are the most one dictation takes.
			s.post(func(*ui.Context) {
				if !s.ended {
					s.stop()
				}
			})
			break
		}
		if len(audio) >= decoded+speech.SampleRate && time.Since(last) >= dictationPartial {
			decoded = len(audio)
			text, err := recognizer.recognize(audio)
			if err != nil {
				return "", dictationKey(err)
			}
			if text != "" && s.ctx.Err() == nil {
				s.post(func(c *ui.Context) { s.insert(c, text) })
			}
			last = time.Now()
		}
	}
	// The device stops before the last frames are taken, so that none
	// arrives behind them.
	failed := source.failed()
	source.stop()
	take()
	closeSource()
	if failed {
		return "", "dictation_microphone"
	}
	if len(audio) == 0 {
		return "", "dictation_no_audio"
	}
	if silentAudio(audio) {
		return "", "dictation_silent"
	}
	text, err := recognizer.recognize(audio)
	if err != nil {
		return "", dictationKey(err)
	}
	return text, ""
}

// silentAudio reports a recording without any sample above the float
// epsilon: a muted or disconnected microphone.
func silentAudio(audio []float32) bool {
	for _, sample := range audio {
		if sample > 1.1920929e-07 || sample < -1.1920929e-07 {
			return false
		}
	}
	return true
}

// meter appends a level to the button's bars.
func (s *dictationSession) meter(level float32) {
	if s.ended || s.view.Phase != composer.Recording {
		return
	}
	levels := append(s.view.Levels, level)
	s.view.Levels = levels[max(len(levels)-dictationLevels, 0):]
}

// complete ends the dictation with its transcript or its failure.
func (s *dictationSession) complete(c *ui.Context, text, key string) {
	if s.ended {
		return
	}
	switch {
	case key != "":
		s.end()
		s.notify(c, key)
	case strings.TrimSpace(text) != "" && !s.readonly():
		s.insert(c, text)
		if !s.ended {
			s.end()
		}
	default:
		s.end()
		s.notify(c, "dictation_empty")
	}
}

// insert puts the transcript at the caret, or in place of the transcript
// put there before. A draft edited meanwhile stops the dictation.
func (s *dictationSession) insert(c *ui.Context, text string) {
	if text == "" || s.ended || s.readonly() {
		return
	}
	start := s.comp.DictationCaret()
	end := start
	if previous := s.partial; previous != nil {
		runes := []rune(s.comp.Draft)
		if previous.end > len(runes) || string(runes[previous.start:previous.end]) != previous.text {
			s.abandon()
			s.notify(c, "dictation_draft_changed")
			return
		}
		start, end = previous.start, previous.end
	}
	if !s.comp.ReplaceDictation(start, end, text) {
		s.abandon()
		s.notify(c, "dictation_draft_changed")
		return
	}
	s.partial = &dictationInsertion{start: start, end: start + len([]rune(text)), text: text}
}
