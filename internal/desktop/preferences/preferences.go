// Package preferences persists controller-owned desktop preferences.
package preferences

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"net"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"sync"
	"unicode"

	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Selection struct {
	Package string `json:"package"`
	Mode    string `json:"mode"`
}
type Terminal struct {
	FontSize        uint8  `json:"font_size"`
	FontFamily      string `json:"font_family"`
	PasteProtection bool   `json:"paste_protection"`
}

// TerminalFontSize is the terminals' text size in DIPs, 8 to 32
// (terminal.font_size).
func (d *Data) TerminalFontSize() float32 { return float32(d.Terminal.FontSize) }

// TerminalFontFamily is the terminals' font family without surrounding
// space, "" for the platform's monospace font (terminal.font_family).
func (d *Data) TerminalFontFamily() string { return strings.TrimSpace(d.Terminal.FontFamily) }

// TerminalPasteProtection reports whether a terminal confirms a paste of
// several lines before sending it (terminal.paste_protection).
func (d *Data) TerminalPasteProtection() bool { return d.Terminal.PasteProtection }

type Device struct {
	Name      *string `json:"name"`
	Execution bool    `json:"execution"`
	Platform  *string `json:"platform"`
}
type Surfaces struct {
	Main    float32  `json:"main"`
	Sidebar *float32 `json:"sidebar"`
	// Blur is an optional Go presentation extension, in percent.
	Blur *float32 `json:"blur,omitempty"`
}
type Composer struct {
	Model      *Model              `json:"model"`
	Mode       protocol.WorkMode   `json:"mode"`
	Permission protocol.Permission `json:"permission"`
	// Team is the team the last message was sent with, "" for one model:
	// a new conversation starts as the last one did, and a new profile
	// with one model.
	Team string `json:"team,omitempty"`
}
type Model struct {
	Source   protocol.NodeID     `json:"source"`
	Provider protocol.ProviderID `json:"provider"`
	Model    string              `json:"model"`
	Effort   protocol.Effort     `json:"effort"`
	Features map[string]string   `json:"features"`
}
type Dictation struct {
	Microphone *string           `json:"microphone"`
	Language   string            `json:"language"`
	Model      *protocol.ModelID `json:"model"`
}
type Pin struct {
	Pinned bool    `json:"pinned"`
	Order  *uint64 `json:"order"`
}
type ArtifactField struct {
	Name   string   `json:"name"`
	Values []string `json:"values"`
}
type ArtifactDraft struct {
	Form   string          `json:"form"`
	Fields []ArtifactField `json:"fields"`
}
type Directory struct {
	Digest          string `json:"digest"`
	InstallRevision uint64 `json:"install_revision"`
	Path            string `json:"path"`
}
type FilePicker struct {
	Grid   bool `json:"grid"`
	Hidden bool `json:"hidden"`
}

// Data preserves the v1 controller format, including nullable later-phase fields.
type Data struct {
	Version           uint32                    `json:"version"`
	Notifications     [4]bool                   `json:"notifications"`
	ToastSeconds      uint64                    `json:"toast_seconds"`
	Pairing           string                    `json:"pairing"`
	IrohRelays        *[]string                 `json:"iroh_relays"`
	Devices           *map[string]Device        `json:"devices"`
	Terminal          Terminal                  `json:"terminal"`
	Appearance        Selection                 `json:"appearance"`
	ThemeEffects      *bool                     `json:"theme_effects"`
	Composer          *Composer                 `json:"composer"`
	FeaturePins       *map[string]bool          `json:"feature_pins"`
	FeatureOrder      *[]string                 `json:"feature_order"`
	BrowserPersistent *bool                     `json:"browser_persistent"`
	Surfaces          *Surfaces                 `json:"surfaces"`
	Shortcuts         *map[string]string        `json:"shortcuts"`
	Dictation         *Dictation                `json:"dictation"`
	Pins              *map[string]Pin           `json:"pins"`
	ArtifactDrafts    *map[string]ArtifactDraft `json:"artifact_drafts"`
	PluginDirectories *map[string]Directory     `json:"plugin_directories"`
	MessageDisplay    *string                   `json:"message_display"`
	SessionOrder      *string                   `json:"session_order"`
	MessageTextSize   *uint8                    `json:"message_text_size"`
	FilePicker        *FilePicker               `json:"file_picker"`
	Language          *string                   `json:"language"`
	UpdateChannel     *string                   `json:"update_channel"`
	WelcomeSeen       *bool                     `json:"welcome_seen"`
	SidebarMetrics    bool                      `json:"sidebar_metrics"`
	// DemoMode is a Go desktop extension: unset, the file stays what the
	// reference writes.
	DemoMode bool `json:"demo_mode,omitempty"`
}

func Default() Data {
	return Data{Version: 1, Notifications: [4]bool{true, true, true, true}, ToastSeconds: 5,
		Terminal: Terminal{FontSize: 14, PasteProtection: true}, Appearance: Selection{Package: "builtin", Mode: "system"}}
}

func (d *Data) UnmarshalJSON(b []byte) error {
	type plain Data
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(b, &fields); err != nil {
		return err
	}
	for _, key := range []string{"version", "notifications", "toast_seconds", "pairing", "terminal", "appearance"} {
		if value, ok := fields[key]; !ok || string(value) == "null" {
			return fmt.Errorf("missing desktop preference %s", key)
		}
	}
	var flags []bool
	if err := json.Unmarshal(fields["notifications"], &flags); err != nil {
		return err
	}
	if len(flags) != 4 {
		return errors.New("invalid notification preferences")
	}
	var value plain
	if err := protocol.DecodeObject(b, &value, []string{"version", "notifications", "toast_seconds", "pairing", "terminal", "appearance"}, false); err != nil {
		return err
	}
	if _, ok := fields["sessions"]; ok {
		if _, exists := fields["pins"]; exists {
			return errors.New("duplicate pins preference")
		}
		var alias struct {
			Sessions *map[string]Pin `json:"sessions"`
		}
		if err := protocol.DecodeObject(b, &alias, nil, false); err != nil {
			return err
		}
		value.Pins = alias.Sessions
	}
	*d = Data(value)
	return d.Validate()
}

var packageID = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{0,63}$`)

// DictationLanguages are the values of dictation.language.
var DictationLanguages = []string{"auto", "chinese", "english"}

// DictationModel is the selected speech model, nil for the recommended one.
// The dictation getters read Data and belong to the thread that edits it.
func (d *Data) DictationModel() *protocol.ModelID {
	if d.Dictation == nil {
		return nil
	}
	return d.Dictation.Model
}

// DictationLanguage is the transcription language, one of DictationLanguages.
func (d *Data) DictationLanguage() string {
	if d.Dictation == nil || d.Dictation.Language == "" {
		return DictationLanguages[0]
	}
	return d.Dictation.Language
}

// DictationMicrophone is the chosen input device's identifier, nil for the
// system default.
func (d *Data) DictationMicrophone() *string {
	if d.Dictation == nil {
		return nil
	}
	return d.Dictation.Microphone
}

var Languages = []string{"system", "english", "chinese", "traditional_chinese", "japanese", "korean", "french", "german", "spanish", "portuguese_brazil", "russian"}

func (d Data) Validate() error {
	if d.Version != 1 {
		return errors.New("unsupported desktop preferences version")
	}
	if !packageID.MatchString(d.Appearance.Package) || !slices.Contains([]string{"system", "light", "dark"}, d.Appearance.Mode) {
		return errors.New("invalid theme selection")
	}
	if !slices.Contains([]uint64{3, 5, 8, 10}, d.ToastSeconds) {
		return errors.New("invalid notification duration")
	}
	if d.Terminal.FontSize < 8 || d.Terminal.FontSize > 32 || strings.ContainsFunc(d.Terminal.FontFamily, unicode.IsControl) {
		return errors.New("invalid terminal display preferences")
	}
	if d.MessageTextSize != nil && (*d.MessageTextSize < 12 || *d.MessageTextSize > 18) {
		return errors.New("invalid conversation text size")
	}
	if d.Surfaces != nil {
		valid := func(v float32, lo float32) bool { return !math.IsNaN(float64(v)) && v >= lo && v <= 100 }
		if !valid(d.Surfaces.Main, 45) || d.Surfaces.Sidebar != nil && !valid(*d.Surfaces.Sidebar, 45) || d.Surfaces.Blur != nil && !valid(*d.Surfaces.Blur, 0) {
			return errors.New("invalid panel opacity")
		}
	}
	for _, choice := range []struct {
		value   *string
		allowed []string
	}{
		{d.Language, Languages}, {d.MessageDisplay, []string{"detailed", "compact"}}, {d.SessionOrder, []string{"created", "updated", "name"}}, {d.UpdateChannel, []string{"stable", "experience"}},
	} {
		if choice.value != nil && !slices.Contains(choice.allowed, *choice.value) {
			return errors.New("invalid desktop preference choice")
		}
	}
	checkURL := func(value string, pairing bool) error {
		u, err := url.Parse(value)
		if err != nil || u.Hostname() == "" || u.User != nil || u.RawQuery != "" || u.ForceQuery || strings.Contains(value, "#") {
			return errors.New("invalid relay URL")
		}
		ip := net.ParseIP(u.Hostname())
		loopback := ip != nil && ip.IsLoopback()
		if u.Scheme != "https" && !(pairing && u.Scheme == "http" && loopback) || pairing && u.Path != "" && u.Path != "/" {
			return errors.New("invalid relay URL")
		}
		return nil
	}
	if d.Pairing != "" {
		if err := checkURL(d.Pairing, true); err != nil {
			return err
		}
	}
	if d.IrohRelays != nil {
		if len(*d.IrohRelays) < 1 || len(*d.IrohRelays) > 8 {
			return errors.New("expected one to eight relay URLs")
		}
		for _, value := range *d.IrohRelays {
			if err := checkURL(value, false); err != nil {
				return err
			}
		}
	}
	return nil
}

func (d Data) Materials() theme.Surfaces {
	s := theme.DefaultSurfaces
	if d.Surfaces != nil {
		s.Main = d.Surfaces.Main / 100
		s.Sidebar = 0.45
		if d.Surfaces.Sidebar != nil {
			s.Sidebar = *d.Surfaces.Sidebar / 100
		}
		if d.Surfaces.Blur != nil {
			s.Blur = *d.Surfaces.Blur / 100
		}
	}
	return s
}

type Store struct {
	Data         Data
	Path         string
	Readable     bool
	Err          error
	startupSave  bool
	mu           sync.Mutex
	writeMu      sync.Mutex
	version      uint64
	nextObserver uint64
	observers    map[uint64]func()
}

func Open(path string) *Store {
	s := &Store{Data: Default(), Path: path, Readable: true}
	b, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return s
	}
	if err == nil {
		err = json.Unmarshal(b, &s.Data)
	}
	if err != nil {
		s.Data = Default()
		s.Readable = false
		s.Err = fmt.Errorf("read desktop preferences: %w", err)
	}
	return s
}

// ForStartup must run before Node creates storage, to detect the first launch.
func ForStartup(profile string) *Store {
	path := filepath.Join(profile, "desktop", "preferences.json")
	existing := false
	for _, p := range []string{path, filepath.Join(profile, "storage", "node.turso"), filepath.Join(profile, "storage", "node.sqlite")} {
		if _, err := os.Stat(p); !errors.Is(err, os.ErrNotExist) {
			existing = true
		}
	}
	s := Open(path)
	if s.Readable && s.Data.WelcomeSeen == nil {
		s.Data.WelcomeSeen = &existing
		s.startupSave = true
	}
	return s
}

func (s *Store) FinishStartup() error {
	if !s.startupSave {
		return s.Err
	}
	s.startupSave = false
	return s.Save()
}

// Save keeps the editable Data on failure and never overwrites unreadable data.
func (s *Store) Save() error {
	if !s.Readable {
		return s.Err
	}
	if err := s.Data.Validate(); err != nil {
		s.Err = err
		return err
	}
	b, err := json.MarshalIndent(s.Data, "", "  ")
	if err != nil {
		return err
	}
	err = s.WriteSnapshot(s.Changed(), b)
	s.Err = err
	return err
}

// WriteSnapshot writes already validated immutable bytes captured by a UI frame.
// It does not mutate editable Data or error state from the background worker.
func (s *Store) WriteSnapshot(version uint64, b []byte) error {
	if !s.Readable {
		return s.Err
	}
	s.writeMu.Lock()
	defer s.writeMu.Unlock()
	if version != s.Revision() {
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(s.Path), 0o700); err != nil {
		return err
	}
	f, err := os.CreateTemp(filepath.Dir(s.Path), ".preferences-")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	if _, err = f.Write(b); err == nil {
		err = f.Sync()
	}
	closeErr := f.Close()
	if err == nil {
		err = closeErr
	}
	if err == nil {
		err = os.Rename(f.Name(), s.Path)
	}
	return err
}

// Revision and Changed coordinate one profile authority across windows. Data
// edits stay on the UI thread; disk workers only receive immutable snapshots.
func (s *Store) Revision() uint64 { s.mu.Lock(); defer s.mu.Unlock(); return s.version }
func (s *Store) Changed() uint64 {
	s.mu.Lock()
	s.version++
	version := s.version
	callbacks := make([]func(), 0, len(s.observers))
	for _, f := range s.observers {
		callbacks = append(callbacks, f)
	}
	s.mu.Unlock()
	for _, f := range callbacks {
		f()
	}
	return version
}
func (s *Store) OnChange(f func()) func() {
	s.mu.Lock()
	if s.observers == nil {
		s.observers = map[uint64]func(){}
	}
	s.nextObserver++
	id := s.nextObserver
	s.observers[id] = f
	s.mu.Unlock()
	return func() { s.mu.Lock(); delete(s.observers, id); s.mu.Unlock() }
}

// PluginDirectory is the local directory a Node's package was installed from,
// while the Node still holds the content that installation gave it.
func (d Data) PluginDirectory(node protocol.NodeID, name, digest string) (string, bool) {
	if d.PluginDirectories == nil {
		return "", false
	}
	directory, known := (*d.PluginDirectories)[PinKey(node, name)]
	return directory.Path, known && directory.Digest == digest
}

// RememberPluginDirectory records the directory an installation came from. A
// record of a later installation stays.
func (d *Data) RememberPluginDirectory(node protocol.NodeID, name string, directory Directory) {
	key := PinKey(node, name)
	if d.PluginDirectories == nil {
		d.PluginDirectories = &map[string]Directory{}
	} else if known, exists := (*d.PluginDirectories)[key]; exists && known.InstallRevision > directory.InstallRevision {
		return
	}
	(*d.PluginDirectories)[key] = directory
}

// ForgetPluginDirectory drops the record once the package was removed or
// installed from elsewhere at the revision. A record of a later installation
// stays.
func (d *Data) ForgetPluginDirectory(node protocol.NodeID, name string, revision uint64) {
	if d.PluginDirectories == nil {
		return
	}
	key := PinKey(node, name)
	if known, exists := (*d.PluginDirectories)[key]; exists && known.InstallRevision <= revision {
		delete(*d.PluginDirectories, key)
	}
}

func PinKey(node protocol.NodeID, session string) string {
	return hex.EncodeToString(node[:]) + "/" + session
}
func (d Data) Pin(node protocol.NodeID, session string) Pin {
	if d.Pins == nil {
		return Pin{}
	}
	return (*d.Pins)[PinKey(node, session)]
}
func (d *Data) SetPin(node protocol.NodeID, session string, pin Pin) {
	if d.Pins == nil {
		pins := map[string]Pin{}
		d.Pins = &pins
	}
	key := PinKey(node, session)
	if !pin.Pinned {
		delete(*d.Pins, key)
		return
	}
	if pin.Order == nil {
		var next uint64
		prefix := hex.EncodeToString(node[:]) + "/"
		for key, state := range *d.Pins {
			if strings.HasPrefix(key, prefix) && state.Pinned && state.Order != nil {
				if *state.Order >= next {
					next = *state.Order
					if next < math.MaxUint64 {
						next++
					}
				}
			}
		}
		pin.Order = &next
	}
	(*d.Pins)[key] = pin
}
