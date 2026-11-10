package plugins

import (
	"context"
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type hostChangeState struct {
	cursors       [4]uint64
	updated       [4]time.Time
	connected     bool
	entry         uint64
	visible       bool
	terminalTools uint64
}

type hostChanges struct {
	owner      *Host
	mu         sync.Mutex
	state      hostChangeState
	changed    chan struct{}
	release    func()
	repository func()
	files      func()
}

func newHostChanges(owner *Host) *hostChanges {
	return &hostChanges{owner: owner, changed: make(chan struct{}), state: hostChangeState{visible: true}}
}

func (changes *hostChanges) start() {
	owner := changes.owner
	if owner.binding.Client != nil {
		var observing sync.Mutex
		observing.Lock()
		defer observing.Unlock()
		observe := func(initial bool) {
			snapshot := owner.binding.Client.Snapshot()
			cursors := owner.binding.Client.PluginCursors(owner.binding.Scope.Package.Name, owner.binding.Actions, owner.binding.Scope.Worktree)
			connected := owner.binding.Client.Connected()
			owner.Observe(cursors.Cursor, cursors.Package, cursors.Settled, connected)
			owner.ObserveTerminalTools(cursors.TerminalTools)
			if !initial || connected {
				owner.ObserveSettings(snapshot.Plugins, connected)
			}
		}
		changes.release = owner.binding.Client.OnChange(func() {
			observing.Lock()
			defer observing.Unlock()
			observe(false)
		})
		observe(true)
	}
}

func (changes *hostChanges) signal() {
	close(changes.changed)
	changes.changed = make(chan struct{})
}

func (changes *hostChanges) close() {
	changes.mu.Lock()
	release, repository, files := changes.release, changes.repository, changes.files
	changes.release, changes.repository, changes.files = nil, nil, nil
	changes.signal()
	changes.mu.Unlock()
	for _, close := range []func(){release, repository, files} {
		if close != nil {
			close()
		}
	}
}

func (owner *Host) Observe(cursor, packageCursor, settled uint64, connected bool) {
	if owner.changes == nil {
		return
	}
	owner.changes.mu.Lock()
	defer owner.changes.mu.Unlock()
	changed := owner.changes.state.connected != connected
	cursors := [4]uint64{cursor, packageCursor, settled, settled}
	for index, cursor := range cursors {
		if owner.changes.state.cursors[index] != cursor {
			owner.changes.state.cursors[index] = cursor
			owner.changes.state.updated[index] = owner.binding.Clock.Now()
			changed = true
		}
	}
	owner.changes.state.connected = connected
	if changed {
		owner.changes.signal()
	}
}

func (owner *Host) ObserveTerminalTools(revision uint64) {
	owner.changes.mu.Lock()
	defer owner.changes.mu.Unlock()
	if owner.changes.state.terminalTools != revision {
		owner.changes.state.terminalTools = revision
		owner.changes.signal()
	}
}

func (owner *Host) SetVisible(visible bool) {
	owner.changes.mu.Lock()
	defer owner.changes.mu.Unlock()
	if owner.changes.state.visible != visible {
		owner.changes.state.visible = visible
		owner.changes.signal()
	}
}

func (owner *Host) Enter() {
	owner.changes.mu.Lock()
	defer owner.changes.mu.Unlock()
	owner.changes.state.entry++
	owner.changes.signal()
}

func (changes *hostChanges) watch(repository bool) error {
	if changes.owner.binding.Client == nil {
		return hostUnavailable("plugin Client is unavailable")
	}
	worktree := changes.owner.scope().Worktree
	if worktree == nil {
		return hostInvalid("change scope requires a worktree")
	}
	changes.mu.Lock()
	defer changes.mu.Unlock()
	current := &changes.files
	if repository {
		current = &changes.repository
	}
	if *current != nil {
		return nil
	}
	release, err := changes.owner.binding.Client.ObserveWorkspace(changes.owner.ctx, *worktree, repository, func(protocol.Update) {
		changes.mu.Lock()
		changes.signal()
		changes.mu.Unlock()
	})
	if err == nil {
		*current = release
	}
	return err
}

func (changes *hostChanges) next(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	seen, err := hostOptional(arguments, 0, "")
	if err != nil {
		return nil, err
	}
	scope := 0
	if len(arguments) > 1 {
		name, err := hostValue[string](arguments, 1)
		if err != nil {
			return nil, err
		}
		switch name {
		case "package":
			scope = 1
		case "settled":
			scope = 2
		case "repository":
			scope = 3
		default:
			return nil, hostInvalid("invalid change scope")
		}
	}
	if scope >= 2 && changes.owner.scope().Worktree == nil {
		return nil, hostInvalid("change scope requires a worktree")
	}
	if scope == 3 {
		if err := changes.watch(true); err != nil {
			return nil, err
		}
	}
	var first, fileUpdated time.Time
	var fileRevision *uint64
	for {
		if err := changes.owner.check(ctx); err != nil {
			return nil, err
		}
		changes.mu.Lock()
		state, changed := changes.state, changes.changed
		changes.mu.Unlock()
		now := changes.owner.binding.Clock.Now()
		var revision, references uint64
		if scope == 3 {
			file, _ := changes.owner.binding.Client.Workspace(*changes.owner.scope().Worktree, true)
			state.connected = state.connected && file.Connected
			revision, references = file.Revision, file.References
			if fileRevision == nil || *fileRevision != revision {
				copy := revision
				fileRevision, fileUpdated = &copy, now
			}
			if fileUpdated.After(state.updated[scope]) {
				state.updated[scope] = fileUpdated
			}
		}
		suffix := fmt.Sprintf("%t:%d:%t", state.connected, state.entry, state.visible)
		cursor := fmt.Sprintf("%d-%d:%s", state.cursors[scope], revision, suffix)
		_, previous, _ := strings.Cut(seen, ":")
		urgent := seen == "" || previous != suffix
		var deadline time.Time
		if cursor != seen && (state.visible || urgent) {
			if first.IsZero() {
				first = now
			}
			deadline = minTime(state.updated[scope].Add(750*time.Millisecond), first.Add(time.Second))
			if urgent || !now.Before(deadline) {
				return hostJSON(map[string]any{
					"cursor": cursor, "revision": strconv.FormatUint(state.cursors[scope], 10),
					"connected": state.connected, "entry": strconv.FormatUint(state.entry, 10), "visible": state.visible,
					"terminal_tools": strconv.FormatUint(state.terminalTools, 10), "references": strconv.FormatUint(references, 10),
				})
			}
		} else {
			first = time.Time{}
		}
		var timeout <-chan time.Time
		var stop func()
		if !deadline.IsZero() {
			timer := changes.owner.binding.Clock.NewTimer(deadline.Sub(now))
			timeout, stop = timer.Chan(), func() { timer.Stop() }
		}
		select {
		case <-ctx.Done():
			if stop != nil {
				stop()
			}
			return nil, ctx.Err()
		case <-changes.owner.ctx.Done():
			if stop != nil {
				stop()
			}
			return nil, changes.owner.ctx.Err()
		case <-changed:
		case <-timeout:
		}
		if stop != nil {
			stop()
		}
	}
}

func minTime(left, right time.Time) time.Time {
	if left.Before(right) {
		return left
	}
	return right
}

func (changes *hostChanges) nextFiles(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
	if !changes.owner.binding.ReadFiles {
		return nil, hostInvalid("plugin does not declare file reading")
	}
	seen, err := hostValue[string](arguments, 0)
	if err != nil {
		return nil, err
	}
	if err := changes.watch(false); err != nil {
		return nil, err
	}
	for {
		changes.mu.Lock()
		changed := changes.changed
		changes.mu.Unlock()
		state, _ := changes.owner.binding.Client.Workspace(*changes.owner.scope().Worktree, false)
		encoded, err := json.Marshal(map[string]any{"revision": state.Revision, "connected": state.Connected})
		if err != nil {
			return nil, err
		}
		if string(encoded) != seen {
			return hostJSON(string(encoded))
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-changes.owner.ctx.Done():
			return nil, changes.owner.ctx.Err()
		case <-changed:
		}
	}
}
