package updater

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"sync"
)

// Kind is where an update stands.
type Kind int

const (
	Idle Kind = iota
	Checking
	Available
	Downloading
	Ready
	Installing
	Restarting
	Stopped
)

// State is the update's state: the version it concerns and, while
// downloading, its progress.
type State struct {
	Kind     Kind
	Version  string
	Progress Progress
}

// Notice is one outcome to show as a toast. A ready notice carries the
// version to install; an error notice carries the failure's detail.
type Notice struct {
	Key     string
	Error   bool
	Version string
	Detail  string
}

// Options binds the service to the running Desktop.
type Options struct {
	Version   string         // the running application version
	Directory string         // the updater's cache, beside the preferences
	Profile   string         // the local Node's data profile
	Channel   func() Channel // the saved channel
	Stop      func() error   // stops the local Node before replacement
	Quit      func()         // ends the application after a handoff
	Changed   func()         // wakes the interface from background work
}

// Service is the update state machine. Its methods return at once; network
// and file work run in the background and report through Snapshot and
// Notices.
type Service struct {
	options Options

	mu          sync.Mutex
	state       State
	failure     *Failure
	selection   *Selection
	config      *Config
	staged      *staged
	restart     *restart
	stop        context.CancelFunc
	operation   uint64
	receiptRead bool
	notices     []Notice
	work        sync.WaitGroup

	// Replaced by tests.
	release   func(channel Channel, version string) (Config, error)
	installed func() (string, error)
	relaunch  func(executable string, arguments []string) error
	client    func(*Config) *http.Client
}

type restart struct {
	executable string
	arguments  []string
}

// New creates an idle service.
func New(options Options) *Service {
	return &Service{options: options, stop: func() {}, release: ReleaseConfig, installed: installedBundle, relaunch: relaunch, client: newClient}
}

// Snapshot is the current state and the last failure.
func (s *Service) Snapshot() (State, *Failure) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.state, s.failure
}

// Notices takes the outcomes not yet shown.
func (s *Service) Notices() []Notice {
	s.mu.Lock()
	defer s.mu.Unlock()
	notices := s.notices
	s.notices = nil
	return notices
}

// Close cancels background work, waits for it and drops a staged download.
func (s *Service) Close() {
	s.mu.Lock()
	s.stop()
	s.operation++
	s.mu.Unlock()
	s.work.Wait()
	s.mu.Lock()
	s.dropStaged()
	s.mu.Unlock()
}

func (s *Service) changed() {
	if s.options.Changed != nil {
		s.options.Changed()
	}
}

func (s *Service) notice(key string, isError bool) {
	s.notices = append(s.notices, Notice{Key: key, Error: isError})
}

func (s *Service) failed(err error) {
	f := asFailure(err)
	log.Printf("desktop update failed: %s: %s", f.Key, f.Detail)
	s.failure = f
	s.notices = append(s.notices, Notice{Key: f.Key, Error: true, Detail: f.Detail})
}

func (s *Service) dropStaged() {
	if s.staged != nil {
		_ = os.RemoveAll(s.staged.directory)
		s.staged = nil
	}
}

// resting is the state an ended check or download returns to.
func (s *Service) resting() State {
	if s.selection != nil {
		return State{Kind: Available, Version: s.selection.Release.Version}
	}
	return State{}
}

// Startup acknowledges the previous installation's receipt once.
func (s *Service) Startup() {
	s.mu.Lock()
	if s.receiptRead {
		s.mu.Unlock()
		return
	}
	s.receiptRead = true
	s.work.Add(1)
	s.mu.Unlock()
	go func() {
		defer s.work.Done()
		installed, _ := s.installed()
		key, err := readReceipt(s.options.Directory, installed, s.options.Version)
		s.mu.Lock()
		if err != nil {
			s.failed(err)
		} else if key != "" {
			s.notice(key, false)
		}
		s.mu.Unlock()
		s.changed()
	}()
}

// Check looks for a newer release on the saved channel.
func (s *Service) Check() { s.check("") }

// CheckVersion looks for the exact version a newer Host asks for.
func (s *Service) CheckVersion(version string) { s.check(version) }

func (s *Service) check(required string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.state.Kind != Idle && s.state.Kind != Available && !(required != "" && s.state.Kind == Ready) {
		return
	}
	s.stop()
	ctx, stop := context.WithCancel(context.Background())
	s.stop = stop
	s.failure = nil
	if required != "" {
		s.selection, s.config = nil, nil
		s.dropStaged()
	}
	s.operation++
	operation := s.operation
	channel := s.options.Channel()
	if required != "" {
		channel = InitialChannel(required)
	}
	s.state = State{Kind: Checking}
	s.work.Add(1)
	go func() {
		defer s.work.Done()
		defer stop()
		config, err := s.release(channel, s.options.Version)
		var selection *Selection
		if err == nil {
			config.Required = required
			selection, err = check(ctx, s.client(&config), &config)
		}
		s.mu.Lock()
		if operation == s.operation {
			if err != nil {
				s.state = s.resting()
				if asFailure(err).Key != "updates_cancelled" {
					s.failed(err)
				}
			} else {
				s.config, s.selection = &config, selection
				s.state = s.resting()
				if selection == nil {
					s.notice("updates_current", false)
				}
			}
		}
		s.mu.Unlock()
		s.changed()
	}()
}

// Download stages and verifies the available release.
func (s *Service) Download() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.state.Kind != Available || s.config == nil || s.selection == nil {
		return
	}
	if s.options.Directory == "" {
		s.notice("updates_io_failed", true)
		return
	}
	s.stop()
	ctx, stop := context.WithCancel(context.Background())
	s.stop = stop
	s.failure = nil
	s.operation++
	operation := s.operation
	config, selection := *s.config, s.selection
	s.state = State{Kind: Downloading, Version: selection.Release.Version, Progress: Progress{Total: selection.Release.Size}}
	s.work.Add(1)
	go func() {
		defer s.work.Done()
		defer stop()
		result, err := download(ctx, s.client(&config), &config, selection, s.options.Directory, func(progress Progress) {
			s.mu.Lock()
			if operation == s.operation {
				s.state.Progress = progress
			}
			s.mu.Unlock()
			s.changed()
		})
		s.mu.Lock()
		switch {
		case operation != s.operation:
			if result != nil {
				_ = os.RemoveAll(result.directory)
			}
		case err != nil:
			s.state = s.resting()
			if asFailure(err).Key != "updates_cancelled" {
				s.failed(err)
			}
		default:
			version := result.selection.Release.Version
			s.state = State{Kind: Ready, Version: version}
			s.staged = result
			s.notices = append(s.notices, Notice{Key: "updates_ready", Version: version})
		}
		s.mu.Unlock()
		s.changed()
	}()
}

// Cancel stops a check or a download.
func (s *Service) Cancel() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.state.Kind == Checking || s.state.Kind == Downloading {
		s.stop()
	}
}

// ChannelChanged abandons everything found on the previous channel and
// reports whether the caller should check again.
func (s *Service) ChannelChanged() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.state.Kind == Installing || s.state.Kind == Restarting || s.state.Kind == Stopped {
		return false
	}
	recheck := s.config != nil || s.state.Kind != Idle
	s.stop()
	s.operation++
	s.selection, s.config, s.failure = nil, nil, nil
	s.dropStaged()
	s.state = State{}
	return recheck
}

// Install replaces the application with the ready release and restarts it.
func (s *Service) Install() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.state.Kind != Ready {
		return
	}
	if err := s.beginInstall(); err != nil {
		s.failed(err)
	}
}

func (s *Service) beginInstall() error {
	update, config := s.staged, s.config
	if update == nil {
		return failure("updates_package_invalid", "no authenticated staged update is available")
	}
	if config == nil {
		return failure("updates_source_unready", "update trust configuration is unavailable")
	}
	installed, err := s.installed()
	if err != nil {
		return err
	}
	root := s.options.Directory
	if root == "" {
		return failure("updates_io_failed", "the controller profile has no cache directory")
	}
	if err := separateData(installed, s.options.Profile, root); err != nil {
		return err
	}
	release := update.selection.Release
	s.failure = nil
	s.state = State{Kind: Installing, Version: release.Version}
	executable := filepath.Join(installed, filepath.FromSlash(release.Executable))
	s.restart = &restart{executable: executable, arguments: os.Args[1:]}
	s.work.Add(1)
	go func() {
		defer s.work.Done()
		stopped, err := s.install(update, config, installed, root)
		if err == nil {
			s.options.Quit()
			return
		}
		s.mu.Lock()
		if stopped {
			// The local Node has drained; only a restart remains.
			s.state = State{Kind: Stopped, Version: release.Version}
		} else {
			s.state = State{Kind: Ready, Version: release.Version}
		}
		s.failed(err)
		s.mu.Unlock()
		s.changed()
	}()
	return nil
}

// install verifies the staged release once more, stops the local Node,
// replaces the bundle and starts the new one. It reports whether the Node
// was stopped.
func (s *Service) install(update *staged, config *Config, installed, root string) (bool, error) {
	release := update.selection.Release
	if err := config.revalidate(update.selection); err != nil {
		return false, err
	}
	if err := verifyArchive(update.archive, release, config.Keys); err != nil {
		return false, err
	}
	if err := validate(filepath.Join(update.directory, "contents", release.Bundle), release); err != nil {
		return false, err
	}
	if err := os.MkdirAll(root, 0o700); err != nil {
		return false, ioFailure(err)
	}
	path := filepath.Join(root, "receipt.json")
	if _, err := os.Lstat(path); err == nil {
		return false, failure("updates_receipt_unread", "an earlier update receipt has not been acknowledged")
	}
	if err := s.options.Stop(); err != nil {
		return true, failure("updates_shutdown_failed", err.Error())
	}
	backup, err := replace(update, config, installed)
	if err != nil {
		f := asFailure(err)
		uncertain := f.Key == "updates_install_uncertain"
		_ = writeReceipt(path, receipt{Version: 1, Result: "failure", Message: &f.Detail, Uncertain: &uncertain})
		return true, f
	}
	if err := writeReceipt(path, receipt{Version: 1, Result: "success", ApplicationVersion: &release.Version, Backup: &backup}); err != nil {
		return true, failure("updates_receipt_failed", fmt.Sprintf("application was installed but the update receipt could not be saved; original retained at %s: %v", backup, err))
	}
	return true, s.relaunch(filepath.Join(installed, filepath.FromSlash(release.Executable)), os.Args[1:])
}

// Restart starts the application again after an installation that stopped
// the local Node could not hand off.
func (s *Service) Restart() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.state.Kind != Stopped {
		return
	}
	target := s.restart
	if target == nil {
		s.notice("updates_restart_failed", true)
		return
	}
	version := s.state.Version
	s.state = State{Kind: Restarting, Version: version}
	s.work.Add(1)
	go func() {
		defer s.work.Done()
		var err error
		if stop := s.options.Stop(); stop != nil {
			err = failure("updates_shutdown_failed", stop.Error())
		} else {
			err = s.relaunch(target.executable, target.arguments)
		}
		if err == nil {
			s.options.Quit()
			return
		}
		s.mu.Lock()
		s.state = State{Kind: Stopped, Version: version}
		s.failed(err)
		s.mu.Unlock()
		s.changed()
	}()
}
