package live

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// The SSH transfer bridge keeps local paths, authenticated streams and the
// original durable publication requests in the application. Scripts receive
// only job snapshots and never a path outside the chosen name or a credential.

const (
	sshTransferReady        = "ready"
	sshTransferPreparing    = "preparing"
	sshTransferTransferring = "transferring"
	sshTransferPublishing   = "publishing"
	sshTransferDone         = "done"
	sshTransferExists       = "exists"
	sshTransferFailed       = "failed"
)

type sshTransferStatus struct {
	stage        string
	copied, size uint64
	fault        *protocol.Fault
}

// sshTransferAccess is the captured execution Node, plugin scope and profile of
// one job. Every Node request of the job carries the plugin scope.
type sshTransferAccess struct {
	owner   *Runtime
	scope   protocol.PluginContext
	profile protocol.SshProfile
}

func (access sshTransferAccess) request(command protocol.Command) protocol.Request {
	request := access.owner.controller.Prepare(command)
	scope := access.scope
	request.Plugin = &scope
	return request
}

func (access sshTransferAccess) execute(ctx context.Context, command protocol.Command) (protocol.Output, error) {
	return access.owner.controller.Execute(ctx, access.request(command))
}

type sshTransferJob struct {
	access    sshTransferAccess
	upload    bool
	local     string
	directory bool
	remote    string

	status sshTransferStatus
	plan   sshTransferPlan
	cancel context.CancelFunc
	busy   bool
}

func (job *sshTransferJob) snapshot(id protocol.RequestID) map[string]any {
	stage, progress := job.status.stage, any(nil)
	var failure any
	unknown := false
	switch job.status.stage {
	case sshTransferTransferring:
		progress = map[string]any{"copied": job.status.copied, "size": job.status.size}
	case sshTransferFailed:
		fault := protocol.Fault{Code: protocol.ErrorCodeUnavailable}
		if job.status.fault != nil {
			fault = *job.status.fault
		}
		failure = fault
		switch fault.Code {
		case protocol.ErrorCodeOutcomeUnknown:
			stage, unknown = "uncertain", true
		case protocol.ErrorCodeCancelled:
			stage = "cancelled"
		}
	}
	kind, source := "upload", filepath.Base(job.local)
	if !job.upload {
		kind, source = "download", job.remote
		if job.directory {
			kind = "download_directory"
		}
	}
	return map[string]any{
		"id": id, "profile": job.access.profile.ID, "kind": kind, "source": source, "path": job.remote,
		"stage": stage, "error": failure, "progress": progress,
		"can_start":   !job.busy && (job.status.stage == sshTransferReady || job.status.stage == sshTransferFailed && (!unknown || job.plan.pending == nil)),
		"can_replace": !job.busy && job.status.stage == sshTransferExists,
		"can_check":   !job.busy && job.plan.pending != nil && unknown,
		"can_cancel":  job.busy && job.status.stage != sshTransferPublishing,
	}
}

// sshTransferStore is shared by every plugin view of one desktop window so a
// reopened SSH page keeps its transfers.
type sshTransferStore struct {
	mu      sync.Mutex
	jobs    map[protocol.RequestID]*sshTransferJob
	changed chan struct{}
	uploads chan struct{}
}

func (r *Runtime) sshTransferStore() *sshTransferStore {
	root := r.localRuntime()
	root.mu.Lock()
	defer root.mu.Unlock()
	if root.sshTransfers == nil {
		root.sshTransfers = &sshTransferStore{jobs: map[protocol.RequestID]*sshTransferJob{}, changed: make(chan struct{}), uploads: make(chan struct{}, 2)}
	}
	return root.sshTransfers
}

func (store *sshTransferStore) publish() {
	close(store.changed)
	store.changed = make(chan struct{})
}

func (store *sshTransferStore) insert(access sshTransferAccess, upload bool, local string, directory bool, remote string) protocol.RequestID {
	status := sshTransferStatus{stage: sshTransferReady}
	if !upload {
		if _, err := os.Lstat(local); err == nil {
			status.stage = sshTransferExists
		}
	}
	id := protocol.NewRequestID()
	store.mu.Lock()
	store.jobs[id] = &sshTransferJob{access: access, upload: upload, local: local, directory: directory, remote: remote, status: status}
	store.publish()
	store.mu.Unlock()
	return id
}

// values returns the job snapshots of one Node and package in identifier order
// with the channel that closes on the next change.
func (store *sshTransferStore) values(node protocol.NodeID, owner protocol.PluginReference) ([]map[string]any, <-chan struct{}) {
	store.mu.Lock()
	defer store.mu.Unlock()
	ids := make([]protocol.RequestID, 0, len(store.jobs))
	for id, job := range store.jobs {
		if job.access.owner.controller.Target() == node && job.access.scope.Package == owner {
			ids = append(ids, id)
		}
	}
	slices.SortFunc(ids, func(left, right protocol.RequestID) int { return bytes.Compare(left[:], right[:]) })
	values := make([]map[string]any, 0, len(ids))
	for _, id := range ids {
		values = append(values, store.jobs[id].snapshot(id))
	}
	return values, store.changed
}

func sshTransferUnavailable() error {
	return pluginNativeFault(protocol.ErrorCodeUnavailable, "SSH file transfer is unavailable")
}

// action applies one script action to a job the caller already authorized.
func (store *sshTransferStore) action(id protocol.RequestID, kind string) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	job := store.jobs[id]
	if job == nil {
		return sshTransferUnavailable()
	}
	switch kind {
	case "cancel":
		if job.cancel != nil {
			job.cancel()
		}
		return nil
	case "dismiss":
		if !job.busy {
			delete(store.jobs, id)
			store.publish()
		}
		return nil
	}
	check, replace := kind == "check", kind == "replace"
	allowed := false
	switch job.status.stage {
	case sshTransferReady:
		allowed = kind == "start"
	case sshTransferExists:
		allowed = replace
	case sshTransferFailed:
		if job.status.fault != nil && job.status.fault.Code == protocol.ErrorCodeOutcomeUnknown && job.plan.pending != nil {
			allowed = check
		} else {
			allowed = kind == "start"
		}
	}
	if job.busy || !allowed {
		return pluginNativeFault(protocol.ErrorCodeUnavailable, "file transfer action is unavailable")
	}
	owner := job.access.owner
	ctx, cancel := context.WithCancel(owner.ctx)
	plan := job.plan
	job.cancel, job.busy, job.status, job.plan = cancel, true, sshTransferStatus{stage: sshTransferPreparing}, sshTransferPlan{}
	update := func(status sshTransferStatus) {
		store.mu.Lock()
		job.status = status
		store.publish()
		store.mu.Unlock()
	}
	started := owner.async(func() {
		defer cancel()
		var status sshTransferStatus
		if job.upload {
			select {
			case store.uploads <- struct{}{}:
				plan, status = job.access.upload(ctx, job.local, job.remote, replace, plan, check, update)
				<-store.uploads
			case <-ctx.Done():
				status = sshTransferFailure(sshTransferCancelled())
			}
		} else {
			status = job.access.download(ctx, job.remote, job.local, job.directory, update)
		}
		store.mu.Lock()
		job.plan, job.status, job.busy = plan, status, false
		store.publish()
		store.mu.Unlock()
	})
	if !started {
		cancel()
		job.plan, job.busy = plan, false
		job.status = sshTransferFailure(&protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "file transfer pool is unavailable"})
	}
	store.publish()
	return nil
}

// sshTransferAccess authorizes one bridge call on the UI thread: the package
// must be available with the declared SSH action, its Node connected and, for
// calls that start work, the profile must be the Node's current revision.
func (state *pluginNativeState) sshTransferAccess(profile protocol.SshProfile, write, current bool) (sshTransferAccess, error) {
	required := protocol.PluginActionReadSsh
	if write {
		required = protocol.PluginActionControlSsh
	}
	info, available := state.packageInfo()
	if !available || info.Extension == nil || !slices.Contains(info.Extension.Actions, required) || !state.runtime.controller.Connected() {
		return sshTransferAccess{}, sshTransferUnavailable()
	}
	if current {
		want, err := json.Marshal(profile)
		if err != nil {
			return sshTransferAccess{}, sshTransferUnavailable()
		}
		if !slices.ContainsFunc(state.runtime.snapshot().Ssh, func(saved protocol.SshProfile) bool {
			have, err := json.Marshal(saved)
			return err == nil && bytes.Equal(have, want)
		}) {
			return sshTransferAccess{}, sshTransferUnavailable()
		}
	}
	return sshTransferAccess{owner: state.runtime, scope: state.scope, profile: profile}, nil
}

func (state *pluginNativeState) sshTransferAuthorize(ctx context.Context, profile protocol.SshProfile, write, current bool) (sshTransferAccess, error) {
	var access sshTransferAccess
	_, err := state.update(ctx, func(*ui.Context) (json.RawMessage, error) {
		var err error
		access, err = state.sshTransferAccess(profile, write, current)
		return nil, err
	})
	return access, err
}

// sshTransferPick opens one native picker on the UI thread and waits for its
// answer; a dismissed picker answers with the zero value.
func sshTransferPick[Value any](ctx context.Context, state *pluginNativeState, open func(reply func(Value))) (Value, error) {
	var zero Value
	selected := make(chan Value, 1)
	if !state.post(func(*ui.Context) {
		open(func(value Value) {
			select {
			case selected <- value:
			default:
			}
		})
	}) {
		return zero, sshTransferUnavailable()
	}
	select {
	case <-ctx.Done():
		return zero, sshTransferUnavailable()
	case <-state.native.Context.Done():
		return zero, sshTransferUnavailable()
	case value := <-selected:
		return value, nil
	}
}

func sshTransferJoin(directory, name string) string {
	return strings.TrimRight(directory, "/") + "/" + name
}

func sshTransferRemoteName(remote string) string {
	return remote[strings.LastIndex(remote, "/")+1:]
}

func (store *sshTransferStore) insertUploads(access sshTransferAccess, directory string, paths []string) ([]protocol.RequestID, error) {
	names := make([]string, len(paths))
	for index, source := range paths {
		names[index] = filepath.Base(source)
		if !filepath.IsAbs(source) || !utf8.ValidString(names[index]) || names[index] == string(filepath.Separator) || names[index] == "." {
			return nil, sshTransferUnavailable()
		}
	}
	ids := make([]protocol.RequestID, 0, len(paths))
	for index, source := range paths {
		ids = append(ids, store.insert(access, true, source, false, sshTransferJoin(directory, names[index])))
	}
	return ids, nil
}

func (state *pluginNativeState) sshTransferOperations() {
	operations := state.native.Operations
	store := state.runtime.sshTransferStore()
	operations["sailry/ssh-transfers.uploadDropped"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		profile, err := pluginNativeArgument[protocol.SshProfile](arguments, 0)
		if err != nil {
			return nil, err
		}
		directory, err := pluginNativeArgument[string](arguments, 1)
		if err != nil {
			return nil, err
		}
		handle, err := pluginNativeArgument[string](arguments, 2)
		if err != nil {
			return nil, err
		}
		// A synchronous export already runs on the UI thread; waiting for the
		// window's queue here would wait for this very call.
		access, err := state.sshTransferAccess(profile, true, true)
		if err != nil {
			return nil, err
		}
		paths, err := state.native.TakeDroppedFiles(handle)
		if err != nil {
			return nil, err
		}
		ids, err := store.insertUploads(access, directory, paths)
		if err != nil {
			return nil, err
		}
		return pluginNativeJSON(ids)
	}
	// previewFile opens a remote file in the application's preview dialog.
	// Its bytes stay in the host: the package names the file alone.
	operations["sailry/ssh-transfers.previewFile"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		profile, err := pluginNativeArgument[protocol.SshProfile](arguments, 0)
		if err != nil {
			return nil, err
		}
		path, err := pluginNativeArgument[string](arguments, 1)
		if err != nil {
			return nil, err
		}
		access, err := state.sshTransferAccess(profile, false, true)
		if err != nil {
			return nil, err
		}
		return state.effect(func(*ui.Context) {
			state.runtime.openFilePreview(path, func(ctx context.Context) ([]byte, error) { return access.readPreview(ctx, path) })
		})
	}
	operations["sailry/ssh-transfers.selectUpload"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		profile, err := pluginNativeArgument[protocol.SshProfile](arguments, 0)
		if err != nil {
			return nil, err
		}
		directory, err := pluginNativeArgument[string](arguments, 1)
		if err != nil {
			return nil, err
		}
		if _, err := pluginNativeArgument[string](arguments, 2); err != nil {
			return nil, err
		}
		access, err := state.sshTransferAuthorize(ctx, profile, true, true)
		if err != nil {
			return nil, err
		}
		if state.runtime.attachments == nil || state.runtime.attachments.native.PickFiles == nil {
			return nil, sshTransferUnavailable()
		}
		paths, err := sshTransferPick(ctx, state, state.runtime.attachments.native.PickFiles)
		if err != nil {
			return nil, err
		}
		ids, err := store.insertUploads(access, directory, paths)
		if err != nil {
			return nil, err
		}
		return pluginNativeJSON(ids)
	}
	operations["sailry/ssh-transfers.selectDownload"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		profile, err := pluginNativeArgument[protocol.SshProfile](arguments, 0)
		if err != nil {
			return nil, err
		}
		entries, err := pluginNativeArgument[[]struct {
			Path      string `json:"path"`
			Directory bool   `json:"directory"`
		}](arguments, 1)
		if err != nil {
			return nil, err
		}
		if _, err := pluginNativeArgument[string](arguments, 2); err != nil {
			return nil, err
		}
		access, err := state.sshTransferAuthorize(ctx, profile, false, true)
		if err != nil {
			return nil, err
		}
		if len(entries) == 0 {
			return nil, sshTransferUnavailable()
		}
		ids := []protocol.RequestID{}
		if len(entries) == 1 {
			if state.runtime.attachments == nil || state.runtime.attachments.native.SaveFile == nil {
				return nil, sshTransferUnavailable()
			}
			save := state.runtime.attachments.native.SaveFile
			local, err := sshTransferPick(ctx, state, func(reply func(string)) { save(sshTransferRemoteName(entries[0].Path), reply) })
			if err != nil {
				return nil, err
			}
			if local != "" {
				ids = append(ids, store.insert(access, false, local, entries[0].Directory, entries[0].Path))
			}
			return pluginNativeJSON(ids)
		}
		if state.runtime.pickDirectory == nil {
			return nil, sshTransferUnavailable()
		}
		root, err := sshTransferPick(ctx, state, state.runtime.pickDirectory)
		if err != nil {
			return nil, err
		}
		if root == "" {
			return pluginNativeJSON(ids)
		}
		names := make([]string, len(entries))
		for index, entry := range entries {
			names[index] = sshTransferRemoteName(entry.Path)
			if !sshTransferLocalName(names[index]) {
				return nil, sshTransferUnavailable()
			}
		}
		for index, entry := range entries {
			ids = append(ids, store.insert(access, false, filepath.Join(root, names[index]), entry.Directory, entry.Path))
		}
		return pluginNativeJSON(ids)
	}
	operations["sailry/ssh-transfers.transferAction"] = func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		text, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		kind, err := pluginNativeArgument[string](arguments, 1)
		if err != nil {
			return nil, err
		}
		id, err := protocol.ParseRequestID(text)
		if err != nil {
			return nil, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid file transfer identifier")
		}
		store.mu.Lock()
		job := store.jobs[id]
		store.mu.Unlock()
		if job == nil {
			return nil, sshTransferUnavailable()
		}
		// Synchronous, like uploadDropped: authorize on the calling UI thread.
		access, err := state.sshTransferAccess(job.access.profile, job.upload, kind == "start" || kind == "replace")
		if err != nil {
			return nil, err
		}
		if job.access.owner.controller.Target() != access.owner.controller.Target() || job.access.scope.Package != access.scope.Package {
			return nil, sshTransferUnavailable()
		}
		if err := store.action(id, kind); err != nil {
			return nil, err
		}
		return json.RawMessage("null"), nil
	}
	operations["sailry/ssh-transfers.nextTransfers"] = func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		seen, err := pluginNativeArgument[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		node, owner := state.runtime.controller.Target(), state.scope.Package
		for {
			values, changed := store.values(node, owner)
			body, err := json.Marshal(values)
			if err != nil {
				return nil, err
			}
			digest := blake3.Sum256(body)
			if cursor := hex.EncodeToString(digest[:]); cursor != seen {
				return pluginNativeJSON(map[string]any{"cursor": cursor, "transfers": json.RawMessage(body)})
			}
			select {
			case <-ctx.Done():
				return nil, sshTransferUnavailable()
			case <-state.native.Context.Done():
				return nil, sshTransferUnavailable()
			case <-changed:
			}
		}
	}
}
