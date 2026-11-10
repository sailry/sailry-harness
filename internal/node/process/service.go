package process

import (
	"cmp"
	"context"
	"errors"
	"fmt"
	"os"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type commandEntry struct {
	info          protocol.ProcessInfo
	initial       protocol.ProcessInfo
	launch        Launch
	stop          context.CancelFunc
	done          chan struct{}
	input         chan []byte
	changed       chan struct{}
	outputChanged chan struct{}
	stdout        *retainedStream
	stderr        *retainedStream
	completion    protocol.ProcessCompletion
	failure       error
	background    bool
	promoted      bool
	explicit      bool
	accessed      time.Time
	lastOutput    time.Time
	pending       bool
	settling      bool
}

type commandSubscriber struct {
	session protocol.SessionID
	updates chan []protocol.ProcessInfo
}

type Service struct {
	mu          sync.Mutex
	options     Options
	ctx         context.Context
	cancel      context.CancelFunc
	entries     map[protocol.RequestID]*commandEntry
	subscribers map[*commandSubscriber]struct{}
	budget      outputBudget
	workers     sync.WaitGroup
	closed      bool
	closeOnce   sync.Once
}

func New(options Options) (*Service, error) {
	if options.Context == nil {
		options.Context = context.Background()
	}
	if options.Now == nil {
		options.Now = time.Now
	}
	if options.After == nil {
		options.After = time.After
	}
	if options.Environment.values == nil {
		options.Environment = NewEnvironment(os.Environ())
	}
	if options.Profile != "" {
		if err := clearOutput(options.Profile); err != nil {
			return nil, err
		}
	}
	ctx, cancel := context.WithCancel(options.Context)
	return &Service{options: options, ctx: ctx, cancel: cancel, entries: map[protocol.RequestID]*commandEntry{}, subscribers: map[*commandSubscriber]struct{}{}, budget: outputBudget{limit: totalLimit}}, nil
}

func (launch Launch) Validate() error {
	if strings.TrimSpace(launch.Command) == "" || strings.ContainsRune(launch.Command, 0) || launch.TimeoutMs == 0 || launch.TimeoutMs > uint64((1<<63-1)/int64(time.Millisecond)) {
		return invalid("invalid command or timeout")
	}
	if strings.ContainsFunc(launch.Cwd, unicode.IsControl) {
		return invalid("invalid command working directory")
	}
	if launch.Pty && !launch.Background {
		return invalid("interactive commands require background execution")
	}
	return nil
}

func (s *Service) Start(launch Launch) (protocol.ProcessInfo, error) {
	launch.Background = true
	entry, err := s.begin(launch, true)
	if err != nil {
		return protocol.ProcessInfo{}, err
	}
	return entry.initial, nil
}

func (s *Service) Execute(ctx context.Context, launch Launch) (protocol.ProcessCompletion, error) {
	launch.Background = false
	entry, err := s.begin(launch, false)
	if err != nil {
		return protocol.ProcessCompletion{}, err
	}
	stop := context.AfterFunc(ctx, func() {
		s.mu.Lock()
		if !entry.promoted {
			entry.stop()
		}
		s.mu.Unlock()
	})
	defer stop()
	<-entry.done
	return entry.completion, entry.failure
}

// ExecutePromotable returns the original command result, or transfers that same
// running command to session ownership when the caller's guidance boundary opens.
// CompletionGate, when supplied, opens after the promoted receipt commits.
func (s *Service) ExecutePromotable(ctx context.Context, launch Launch, promotion <-chan struct{}) (protocol.Output, error) {
	launch.Background = false
	entry, err := s.begin(launch, false)
	if err != nil {
		return protocol.Output{}, err
	}
	forward := context.AfterFunc(ctx, func() {
		s.mu.Lock()
		if !entry.promoted {
			entry.stop()
		}
		s.mu.Unlock()
	})
	defer forward()
	select {
	case <-entry.done:
		return protocol.Output{Kind: "command_result", Data: entry.completion}, entry.failure
	case <-promotion:
	}
	s.mu.Lock()
	if entry.settling || !commandActive(entry.info) || ctx.Err() != nil {
		s.mu.Unlock()
		<-entry.done
		return protocol.Output{Kind: "command_result", Data: entry.completion}, entry.failure
	}
	entry.background, entry.promoted = true, true
	info := entry.info
	s.publishLocked(info.Session)
	s.mu.Unlock()
	return protocol.Output{Kind: "command_started", Data: info}, nil
}

func (s *Service) begin(launch Launch, background bool) (*commandEntry, error) {
	if err := launch.Validate(); err != nil {
		return nil, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed || s.ctx.Err() != nil {
		return nil, unavailable("command service is closed")
	}
	if s.entries[launch.ID] != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "command identity already exists"}
	}
	if background {
		active := 0
		for _, entry := range s.entries {
			if entry.background && commandActive(entry.info) {
				active++
			}
		}
		if active >= 32 {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "background command capacity exhausted"}
		}
	}
	s.trimLocked()
	profile := launch.Profile
	if profile == "" {
		profile = s.options.Profile
		launch.Profile = profile
	}
	if profile == "" {
		profile = launch.Root
	}
	ctx, cancel := context.WithCancel(s.ctx)
	entry := &commandEntry{launch: launch, stop: cancel, done: make(chan struct{}), changed: make(chan struct{}), outputChanged: make(chan struct{}), background: background, accessed: s.options.Now()}
	entry.info = protocol.ProcessInfo{ID: launch.ID, Session: launch.Session, Turn: launch.Turn, Command: launch.Command, Access: launch.Access, Pty: launch.Pty, Cwd: launch.Cwd, StartedMs: uint64(max(s.options.Now().UnixMilli(), 0)), Status: protocol.ProcessStatus{Kind: "running"}, Services: []protocol.ProcessService{}}
	entry.initial = entry.info
	if background {
		entry.input = make(chan []byte, 32)
	}
	entry.stdout = newRetained(profile, &s.budget, background, !launch.Pty && s.options.Run == nil)
	entry.stderr = newRetained(profile, &s.budget, background, !launch.Pty && s.options.Run == nil)
	s.entries[launch.ID] = entry
	s.workers.Add(1)
	go s.run(ctx, entry)
	s.publishLocked(launch.Session)
	return entry, nil
}

func (s *Service) run(ctx context.Context, entry *commandEntry) {
	defer s.workers.Done()
	defer entry.stop()
	if entry.launch.Release != nil {
		defer entry.launch.Release()
	}
	run := s.options.Run
	if run == nil {
		run = s.executeNative
	}
	completion, failure := func() (completion protocol.ProcessCompletion, failure error) {
		defer func() {
			if recover() != nil {
				failure = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "command execution failed; side effects were not replayed"}
			}
		}()
		return run(ctx, entry.launch, func(kind protocol.ProcessOutputOutputStream, value []byte) { s.append(entry, kind, value) }, entry.input)
	}()
	entry.stdout.finish()
	entry.stderr.finish()
	completion.Stdout, completion.Stderr = entry.stdout.snapshot(false), entry.stderr.snapshot(false)
	s.mu.Lock()
	entry.settling = true
	info := entry.info
	if failure == nil {
		info.Status = protocol.ProcessStatus{Kind: "finished", Data: completion.Outcome}
	} else {
		info.Status = protocol.ProcessStatus{Kind: "failed", Data: failure.Error()}
	}
	background, explicit := entry.background, entry.explicit
	s.mu.Unlock()
	if background && entry.launch.CompletionGate != nil {
		select {
		case <-entry.launch.CompletionGate:
		case <-s.ctx.Done():
		}
	}
	if background && s.options.Finished != nil {
		elapsed := completion.ElapsedMs
		var timing *uint64
		if failure == nil {
			timing = &elapsed
		}
		finished := Finished{Snapshot: protocol.ProcessSnapshot{Info: info, Stdout: completion.Stdout, Stderr: completion.Stderr}, ElapsedMs: timing, Explicit: explicit}
		if err := s.options.Finished(context.WithoutCancel(s.ctx), finished); err != nil {
			failure = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "command completion could not be recorded; side effects were not replayed: " + err.Error()}
			info.Status = protocol.ProcessStatus{Kind: "failed", Data: failure.Error()}
		}
	}
	s.mu.Lock()
	entry.info, entry.completion, entry.failure = info, completion, failure
	entry.accessed, entry.pending = s.options.Now(), false
	s.changeLocked(entry)
	close(entry.done)
	s.publishLocked(info.Session)
	s.mu.Unlock()
	if s.options.Changed != nil {
		s.options.Changed(entry.launch.Worktree)
	}
}

func (s *Service) append(entry *commandEntry, kind protocol.ProcessOutputOutputStream, value []byte) {
	if kind == protocol.ProcessOutputOutputStreamStderr {
		entry.stderr.append(value)
	} else {
		entry.stdout.append(value)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	services := []protocol.ProcessService{}
	for _, stream := range []*retainedStream{entry.stdout, entry.stderr} {
		for _, service := range stream.services() {
			if len(services) < 16 && !slices.ContainsFunc(services, func(found protocol.ProcessService) bool { return found.Port == service.Port }) {
				services = append(services, service)
			}
		}
	}
	if !slices.Equal(entry.info.Services, services) {
		entry.info.Services = services
		s.publishLocked(entry.info.Session)
	}
	now := s.options.Now()
	if entry.lastOutput.IsZero() || now.Sub(entry.lastOutput) >= 100*time.Millisecond {
		entry.lastOutput = now
		s.outputLocked(entry)
		return
	}
	if entry.pending {
		return
	}
	entry.pending = true
	delay := 100*time.Millisecond - now.Sub(entry.lastOutput)
	go func() {
		select {
		case <-entry.done:
			return
		case <-s.ctx.Done():
			return
		case <-s.options.After(delay):
		}
		s.mu.Lock()
		defer s.mu.Unlock()
		if entry.pending {
			entry.pending, entry.lastOutput = false, s.options.Now()
			s.outputLocked(entry)
		}
	}()
}

func (s *Service) Promote(session protocol.SessionID, id protocol.RequestID) (protocol.ProcessInfo, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	entry, err := s.entryLocked(session, id)
	if err != nil {
		return protocol.ProcessInfo{}, err
	}
	if entry.info.Status.Kind != "running" || entry.settling {
		return protocol.ProcessInfo{}, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "command has already completed"}
	}
	entry.background, entry.promoted = true, true
	s.publishLocked(session)
	return entry.info, nil
}

func (s *Service) List(session protocol.SessionID) []protocol.ProcessInfo {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.listLocked(session)
}

func (s *Service) listLocked(session protocol.SessionID) []protocol.ProcessInfo {
	items := []protocol.ProcessInfo{}
	for _, entry := range s.entries {
		if entry.info.Session == session && entry.background {
			items = append(items, entry.info)
		}
	}
	slices.SortFunc(items, func(a, b protocol.ProcessInfo) int {
		if value := cmp.Compare(a.StartedMs, b.StartedMs); value != 0 {
			return value
		}
		return strings.Compare(a.ID.String(), b.ID.String())
	})
	return items
}

func (s *Service) Read(session protocol.SessionID, id protocol.RequestID) (protocol.ProcessSnapshot, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	entry, err := s.entryLocked(session, id)
	if err != nil {
		return protocol.ProcessSnapshot{}, err
	}
	return s.snapshotLocked(entry), nil
}

func (s *Service) snapshotLocked(entry *commandEntry) protocol.ProcessSnapshot {
	recent := entry.promoted && commandActive(entry.info)
	return protocol.ProcessSnapshot{Info: entry.info, Stdout: entry.stdout.snapshot(recent), Stderr: entry.stderr.snapshot(recent)}
}

func (s *Service) Page(session protocol.SessionID, id protocol.RequestID, kind protocol.ProcessOutputOutputStream, offset, limit uint64) (protocol.ProcessOutputOutputPage, error) {
	s.mu.Lock()
	entry, err := s.entryLocked(session, id)
	if err != nil {
		s.mu.Unlock()
		return protocol.ProcessOutputOutputPage{}, err
	}
	info := entry.info
	stream := entry.stdout
	if kind == protocol.ProcessOutputOutputStreamStderr {
		stream = entry.stderr
	} else if kind != protocol.ProcessOutputOutputStreamStdout {
		s.mu.Unlock()
		return protocol.ProcessOutputOutputPage{}, invalid("command output stream is invalid")
	}
	s.mu.Unlock()
	return stream.page(info, kind, offset, limit)
}

func (s *Service) Stop(session protocol.SessionID, id protocol.RequestID) (protocol.ProcessSnapshot, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	entry, err := s.entryLocked(session, id)
	if err != nil {
		return protocol.ProcessSnapshot{}, err
	}
	if entry.info.Status.Kind == "running" {
		entry.info.Status.Kind = "stopping"
	}
	entry.explicit = true
	entry.stop()
	s.changeLocked(entry)
	s.publishLocked(session)
	return s.snapshotLocked(entry), nil
}

func (s *Service) StopAll(session protocol.SessionID) []protocol.ProcessInfo {
	items := s.List(session)
	for i := range items {
		if snapshot, err := s.Stop(session, items[i].ID); err == nil {
			items[i] = snapshot.Info
		}
	}
	return items
}

func (s *Service) Write(session protocol.SessionID, id protocol.RequestID, input string) (uint64, error) {
	if len(input) > 64*1024 {
		return 0, invalid("command input exceeds the size limit")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	entry, err := s.entryLocked(session, id)
	if err != nil {
		return 0, err
	}
	if entry.info.Status.Kind != "running" {
		return 0, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "command input is closed"}
	}
	if input == "" {
		return 0, nil
	}
	if entry.input == nil {
		return 0, unavailable("command input is closed")
	}
	select {
	case <-entry.done:
		return 0, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "command input closed before delivery; inspect output before retrying"}
	case entry.input <- []byte(input):
		return uint64(len(input)), nil
	default:
		return 0, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "command input queue is full"}
	}
}

func (s *Service) Wait(ctx context.Context, session protocol.SessionID, id protocol.RequestID, waitMs uint64, until string) error {
	var deadline <-chan time.Time
	if waitMs > 0 {
		deadline = s.options.After(time.Duration(min(waitMs, 30000)) * time.Millisecond)
	}
	received := false
	for {
		s.mu.Lock()
		entry, err := s.entryLocked(session, id)
		if err != nil {
			s.mu.Unlock()
			return err
		}
		snapshot, changes, output := s.snapshotLocked(entry), entry.changed, entry.outputChanged
		s.mu.Unlock()
		if waitMs == 0 || !commandActive(snapshot.Info) || until != "" && (strings.Contains(snapshot.Stdout.Text, until) || strings.Contains(snapshot.Stderr.Text, until)) || received && until == "" {
			return nil
		}
		select {
		case <-ctx.Done():
			return unavailable("command output wait closed")
		case <-s.ctx.Done():
			return unavailable("command output wait closed")
		case <-deadline:
			return nil
		case <-changes:
		case <-output:
			received = true
		}
	}
}

func (s *Service) Subscribe(session protocol.SessionID) (<-chan []protocol.ProcessInfo, func()) {
	s.mu.Lock()
	defer s.mu.Unlock()
	watch := &commandSubscriber{session: session, updates: make(chan []protocol.ProcessInfo, 1)}
	watch.updates <- s.listLocked(session)
	if s.closed {
		close(watch.updates)
		return watch.updates, func() {}
	}
	s.subscribers[watch] = struct{}{}
	var once sync.Once
	return watch.updates, func() {
		once.Do(func() {
			s.mu.Lock()
			defer s.mu.Unlock()
			if _, ok := s.subscribers[watch]; ok {
				delete(s.subscribers, watch)
				close(watch.updates)
			}
		})
	}
}

func (s *Service) CloseSession(ctx context.Context, session protocol.SessionID) ([]protocol.ProcessInfo, error) {
	s.mu.Lock()
	var entries []*commandEntry
	for _, entry := range s.entries {
		if entry.info.Session == session {
			entry.stop()
			entries = append(entries, entry)
		}
	}
	s.mu.Unlock()
	for _, entry := range entries {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-entry.done:
		}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	items := s.listLocked(session)
	for _, entry := range entries {
		entry.stdout.evict()
		entry.stderr.evict()
		delete(s.entries, entry.info.ID)
	}
	s.publishLocked(session)
	return items, nil
}

func (s *Service) Close() error {
	s.closeOnce.Do(func() {
		s.mu.Lock()
		s.closed = true
		s.cancel()
		s.mu.Unlock()
		s.workers.Wait()
		s.mu.Lock()
		defer s.mu.Unlock()
		for _, entry := range s.entries {
			entry.stdout.evict()
			entry.stderr.evict()
		}
		for watch := range s.subscribers {
			close(watch.updates)
			delete(s.subscribers, watch)
		}
	})
	return nil
}

func (s *Service) entryLocked(session protocol.SessionID, id protocol.RequestID) (*commandEntry, error) {
	entry := s.entries[id]
	if entry == nil || entry.info.Session != session {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "command is not available in this session"}
	}
	entry.accessed = s.options.Now()
	return entry, nil
}

func (s *Service) changeLocked(entry *commandEntry) {
	close(entry.changed)
	entry.changed = make(chan struct{})
}

func (s *Service) outputLocked(entry *commandEntry) {
	close(entry.outputChanged)
	entry.outputChanged = make(chan struct{})
}

func (s *Service) publishLocked(session protocol.SessionID) {
	for watch := range s.subscribers {
		if watch.session == session {
			select {
			case <-watch.updates:
			default:
			}
			watch.updates <- s.listLocked(session)
		}
	}
}

func (s *Service) trimLocked() {
	var finished []*commandEntry
	for _, entry := range s.entries {
		if !commandActive(entry.info) {
			finished = append(finished, entry)
		}
	}
	slices.SortFunc(finished, func(a, b *commandEntry) int { return a.accessed.Compare(b.accessed) })
	for len(s.entries) >= 128 && len(finished) > 0 {
		entry := finished[0]
		finished = finished[1:]
		entry.stdout.evict()
		entry.stderr.evict()
		delete(s.entries, entry.info.ID)
	}
	needed := int64(commandLimit) - s.budget.available()
	retained := 0
	for _, entry := range finished {
		retained += entry.stdout.bytes() + entry.stderr.bytes()
	}
	if int64(retained) < needed {
		return
	}
	for _, entry := range finished {
		if s.budget.available() >= commandLimit {
			break
		}
		entry.stdout.evict()
		entry.stderr.evict()
	}
}

func commandActive(info protocol.ProcessInfo) bool {
	return info.Status.Kind == "running" || info.Status.Kind == "stopping"
}
func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
func unavailable(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}

func processFailure(err error) error {
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return fault
	}
	return unavailable(fmt.Sprintf("command launch failed: %v", err))
}
