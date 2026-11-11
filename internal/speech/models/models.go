// Package models is the controller's own speech-model installations: the
// recommended SenseVoice model and the models registered in the index
// models/speech.json of the desktop data directory, with their files under
// models/speech/<id>. No Node lists, installs or deletes a speech model.
package models

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/speech"
)

var (
	// ErrStorage reports an index or model directory that cannot be read or
	// written.
	ErrStorage = errors.New("speech model storage failed")
	// ErrBusy reports a model that is recording, transcribing or downloading.
	ErrBusy = errors.New("speech model is in use")
	// ErrMissing reports a model that is not registered or not installed.
	ErrMissing = errors.New("speech model is not installed")
	// ErrConflict reports a definition whose source is registered with
	// other files.
	ErrConflict = errors.New("speech model source is registered with other files")
)

// recommendedSource is the pinned revision of the recommended model.
const recommendedSource = "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07"

// recommendedFiles are the recommended SenseVoice INT8 model's files.
var recommendedFiles = []assets.File{
	{Name: "model.int8.onnx", Size: 239233841, SHA256: "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51"},
	{Name: "tokens.txt", Size: 315894, SHA256: "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc"},
}

// Record is one registered model of the index.
type Record struct {
	ID         protocol.ModelID          `json:"id"`
	Definition protocol.ModelsDefinition `json:"definition"`
	Installed  bool                      `json:"installed"`
}

// Status is where a model's installation is.
type Status int

const (
	Missing Status = iota
	Downloading
	Installed
	Failed
)

// State is a model's installation: the download's progress while
// Downloading and the failure while Failed.
type State struct {
	Status   Status
	Progress assets.Progress
	Err      error
}

// Entry is a registered model with its state.
type Entry struct {
	Record Record
	State  State
}

// Options locate the manager and give it the desktop's outbound transport.
type Options struct {
	// Directory is the desktop data directory.
	Directory string
	// Transport performs downloads and carries the desktop's proxy settings.
	Transport http.RoundTripper
	// Validate loads the downloaded graph and vocabulary before a model
	// counts as installed. A model that cannot be validated is not installed.
	Validate func(graph, tokens string) error
	// Changed is called, outside the manager's lock and on any goroutine,
	// after an entry or a state changed.
	Changed func()
}

type job struct {
	cancel    context.CancelFunc
	cancelled bool
	state     State
}

// Manager owns the index, the downloads and the leases of the models in
// use. A nil model ID names the recommended model throughout.
type Manager struct {
	options Options
	client  *http.Client
	ctx     context.Context

	mu      sync.Mutex
	records []Record
	jobs    map[string]*job
	active  map[string]int
	workers sync.WaitGroup
}

func name(id *protocol.ModelID) string {
	if id == nil {
		return ""
	}
	return id.String()
}

// Open reads the index. Downloads run until ctx ends.
func Open(ctx context.Context, options Options) (*Manager, error) {
	var records []Record
	bytes, err := os.ReadFile(filepath.Join(options.Directory, "models", "speech.json"))
	switch {
	case errors.Is(err, os.ErrNotExist):
	case err != nil:
		return nil, fmt.Errorf("%w: %v", ErrStorage, err)
	default:
		if err := json.Unmarshal(bytes, &records); err != nil {
			return nil, fmt.Errorf("%w: %v", ErrStorage, err)
		}
	}
	seen := map[protocol.ModelID]bool{}
	for index := range records {
		record := &records[index]
		if localmodels.ValidateDefinition(&record.Definition) != nil || record.Definition.Format.Kind != "sense_voice" || seen[record.ID] {
			return nil, fmt.Errorf("%w: invalid speech model index", ErrStorage)
		}
		seen[record.ID] = true
	}
	manager := &Manager{options: options, client: assets.Client(options.Transport), ctx: ctx, records: records, jobs: map[string]*job{}, active: map[string]int{}}
	manager.sweep()
	return manager, nil
}

// sweep clears what a crash left under the models directory: index drafts,
// the holders of interrupted removals and the leftovers of interrupted
// downloads that cannot be resumed. A removal that stopped before its index
// was saved is undone, so that a model the index lists has its files.
// Nothing downloads or is removed before Open returns.
func (m *Manager) sweep() {
	directory := filepath.Join(m.options.Directory, "models")
	drafts, _ := os.ReadDir(directory)
	for _, draft := range drafts {
		if name := draft.Name(); strings.HasPrefix(name, ".speech-") && strings.HasSuffix(name, ".json") {
			_ = os.Remove(filepath.Join(directory, name))
		}
	}
	registered := map[string]bool{}
	for _, record := range m.records {
		registered[record.ID.String()] = true
	}
	holders, _ := os.ReadDir(filepath.Join(directory, "speech"))
	for _, entry := range holders {
		id, found := strings.CutPrefix(entry.Name(), holderPrefix)
		if !found {
			continue
		}
		holder, path := filepath.Join(directory, "speech", entry.Name()), filepath.Join(directory, "speech", id)
		if _, err := os.Lstat(path); registered[id] && errors.Is(err, os.ErrNotExist) {
			_ = os.Rename(filepath.Join(holder, "model"), path)
		}
		_ = os.RemoveAll(holder)
	}
	stale := time.Now().Add(-assets.PartRetention)
	assets.Sweep(m.path(nil), recommendedFiles, stale)
	for index := range m.records {
		record := &m.records[index]
		assets.Sweep(m.path(&record.ID), files(&record.Definition), stale)
	}
}

// holderPrefix names the directory beside a model that keeps its files
// while its removal is saved; the model's ID follows.
const holderPrefix = ".speech-delete-"

// Wait returns when every download started so far has ended.
func (m *Manager) Wait() { m.workers.Wait() }

func (m *Manager) changed() {
	if m.options.Changed != nil {
		m.options.Changed()
	}
}

// Entries are the registered models with their states.
func (m *Manager) Entries() []Entry {
	m.mu.Lock()
	defer m.mu.Unlock()
	entries := make([]Entry, 0, len(m.records))
	for _, record := range m.records {
		state := State{}
		if record.Installed {
			state.Status = Installed
		}
		if job := m.jobs[record.ID.String()]; job != nil {
			state = job.state
		}
		entries = append(entries, Entry{Record: record, State: state})
	}
	return entries
}

// RecommendedState is the recommended model's state once a download or an
// observation set it.
func (m *Manager) RecommendedState() (State, bool) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if job := m.jobs[""]; job != nil {
		return job.state, true
	}
	return State{}, false
}

// Observe records whether a model's files were found complete, unless it
// is downloading.
func (m *Manager) Observe(id *protocol.ModelID, ready bool) {
	state := State{}
	if ready {
		state.Status = Installed
	}
	m.mu.Lock()
	current := m.jobs[name(id)]
	if current != nil && (current.state.Status == Downloading || current.state.Status == state.Status) {
		m.mu.Unlock()
		return
	}
	m.jobs[name(id)] = &job{cancel: func() {}, state: state}
	m.mu.Unlock()
	m.changed()
}

// Capture leases a model for one dictation, so that it is neither removed
// nor downloaded again while it records or transcribes.
type Capture struct {
	manager    *Manager
	key        string
	directory  string
	definition *protocol.ModelsDefinition
	release    sync.Once
}

// Capture leases the selected model. A registered model must be installed;
// a model that is downloading is ErrMissing.
func (m *Manager) Capture(id *protocol.ModelID) (*Capture, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if job := m.jobs[name(id)]; job != nil && job.state.Status == Downloading {
		return nil, ErrMissing
	}
	var definition *protocol.ModelsDefinition
	if id != nil {
		for _, record := range m.records {
			if record.ID == *id && record.Installed {
				definition = &record.Definition
			}
		}
		if definition == nil {
			return nil, ErrMissing
		}
	}
	m.active[name(id)]++
	return &Capture{manager: m, key: name(id), directory: m.path(id), definition: definition}, nil
}

// Ready reports whether every file of the model is present with its pinned
// size and SHA-256. It hashes the files, so it runs off the UI thread.
func (c *Capture) Ready() bool {
	if c.definition != nil {
		return assets.Ready(c.directory, files(c.definition))
	}
	return assets.Ready(c.directory, recommendedFiles)
}

// Paths are the model's graph and vocabulary.
func (c *Capture) Paths() (graph, tokens string) { return paths(c.directory, c.definition != nil) }

// paths names the graph by the model's kind: a registered model's is
// model.onnx and the recommended model's the quantized model.int8.onnx.
func paths(directory string, registered bool) (graph, tokens string) {
	graph = "model.int8.onnx"
	if registered {
		graph = "model.onnx"
	}
	return filepath.Join(directory, graph), filepath.Join(directory, "tokens.txt")
}

// Release ends the lease. It may be called more than once.
func (c *Capture) Release() {
	c.release.Do(func() {
		m := c.manager
		m.mu.Lock()
		if m.active[c.key]--; m.active[c.key] <= 0 {
			delete(m.active, c.key)
		}
		m.mu.Unlock()
	})
}

// Register adds a resolved SenseVoice definition to the index and returns
// its ID, or the ID it already has. The same source with other files is
// ErrConflict.
func (m *Manager) Register(definition protocol.ModelsDefinition) (protocol.ModelID, error) {
	if localmodels.ValidateDefinition(&definition) != nil || definition.Format.Kind != "sense_voice" {
		return protocol.ModelID{}, speech.ErrInvalidModel
	}
	m.mu.Lock()
	for _, record := range m.records {
		if record.Definition.Source == definition.Source {
			m.mu.Unlock()
			if sameDefinition(record.Definition, definition) {
				return record.ID, nil
			}
			return protocol.ModelID{}, ErrConflict
		}
	}
	id := protocol.NewModelID()
	m.records = append(m.records, Record{ID: id, Definition: definition})
	if err := m.save(); err != nil {
		m.records = m.records[:len(m.records)-1]
		m.mu.Unlock()
		return protocol.ModelID{}, err
	}
	m.mu.Unlock()
	m.changed()
	return id, nil
}

func sameDefinition(a, b protocol.ModelsDefinition) bool {
	first, err := json.Marshal(a)
	second, other := json.Marshal(b)
	return err == nil && other == nil && string(first) == string(second)
}

// Cancelling reports a download whose cancellation was asked for.
func (m *Manager) Cancelling(id *protocol.ModelID) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	job := m.jobs[name(id)]
	return job != nil && job.cancelled
}

// Cancel stops a model's download; its partial files stay for a resume.
func (m *Manager) Cancel(id *protocol.ModelID) {
	m.mu.Lock()
	job := m.jobs[name(id)]
	downloading := job != nil && job.state.Status == Downloading
	if downloading {
		job.cancelled = true
	}
	m.mu.Unlock()
	if downloading {
		job.cancel()
		m.changed()
	}
}

// Remove deletes a registered model and its files. A model in use or
// downloading is ErrBusy.
func (m *Manager) Remove(id protocol.ModelID) error {
	m.mu.Lock()
	key := id.String()
	if job := m.jobs[key]; m.active[key] > 0 || job != nil && job.state.Status == Downloading {
		m.mu.Unlock()
		return ErrBusy
	}
	index := -1
	for i, record := range m.records {
		if record.ID == id {
			index = i
		}
	}
	if index < 0 {
		m.mu.Unlock()
		return ErrMissing
	}
	// The files stay recoverable until the index without them is saved.
	path, parked := m.path(&id), ""
	if info, err := os.Lstat(path); err == nil {
		if !info.IsDir() {
			m.mu.Unlock()
			return fmt.Errorf("%w: model directory is not a directory", ErrStorage)
		}
		// Named by the model, so that Open can undo a removal a crash
		// interrupted before the index was saved.
		holder := filepath.Join(filepath.Dir(path), holderPrefix+key)
		err := os.RemoveAll(holder)
		if err == nil {
			err = os.Mkdir(holder, 0o700)
		}
		if err == nil {
			parked = filepath.Join(holder, "model")
			err = os.Rename(path, parked)
		}
		if err != nil {
			m.mu.Unlock()
			return fmt.Errorf("%w: %v", ErrStorage, err)
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		m.mu.Unlock()
		return fmt.Errorf("%w: %v", ErrStorage, err)
	}
	previous := append([]Record(nil), m.records...)
	m.records = append(m.records[:index:index], m.records[index+1:]...)
	if err := m.save(); err != nil {
		m.records = previous
		if parked != "" && os.Rename(parked, path) == nil {
			_ = os.Remove(filepath.Dir(parked))
		}
		m.mu.Unlock()
		return err
	}
	delete(m.jobs, key)
	m.mu.Unlock()
	if parked != "" {
		_ = os.RemoveAll(filepath.Dir(parked))
	}
	m.changed()
	return nil
}

func (m *Manager) path(id *protocol.ModelID) string {
	if id == nil {
		return filepath.Join(m.options.Directory, "models", "sensevoice-int8")
	}
	return filepath.Join(m.options.Directory, "models", "speech", id.String())
}

// save replaces the index atomically. The caller holds the lock.
func (m *Manager) save() error {
	directory := filepath.Join(m.options.Directory, "models")
	bytes, err := json.Marshal(m.records)
	if err == nil {
		err = os.MkdirAll(directory, 0o700)
	}
	if err != nil {
		return fmt.Errorf("%w: %v", ErrStorage, err)
	}
	file, err := os.CreateTemp(directory, ".speech-*.json")
	if err != nil {
		return fmt.Errorf("%w: %v", ErrStorage, err)
	}
	_, err = file.Write(bytes)
	if err == nil {
		err = file.Sync()
	}
	if closed := file.Close(); err == nil {
		err = closed
	}
	if err == nil {
		err = os.Rename(file.Name(), filepath.Join(directory, "speech.json"))
	}
	if err != nil {
		_ = os.Remove(file.Name())
		return fmt.Errorf("%w: %v", ErrStorage, err)
	}
	return nil
}

func files(definition *protocol.ModelsDefinition) []assets.File {
	list := make([]assets.File, 0, len(definition.Assets))
	for _, asset := range definition.Assets {
		list = append(list, assets.File{Name: asset.Name, Size: asset.Bytes, SHA256: asset.Sha256})
	}
	return list
}

func recommendedDownloads() ([]assets.Download, error) {
	downloads := make([]assets.Download, 0, len(recommendedFiles))
	for _, file := range recommendedFiles {
		address, err := url.Parse(recommendedSource + "/" + file.Name)
		if err != nil {
			return nil, speech.ErrInvalidModel
		}
		downloads = append(downloads, assets.Download{File: file, URL: address})
	}
	return downloads, nil
}
