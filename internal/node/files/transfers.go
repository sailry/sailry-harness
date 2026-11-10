package files

import (
	"context"
	"errors"
	"io"
	"math"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const attachmentLimit = 64 << 20
const prepareTimeout = 8 * time.Second
const openTimeout = 60 * time.Second
const idleTimeout = 30 * time.Second

type transferEntry struct {
	caller     protocol.NodeID
	root       string
	state      string
	expires    time.Time
	timer      func() bool
	file       *os.File
	temporary  string
	tempRoot   *os.Root
	files      *core.Files
	target     *core.FileTarget
	upload     *protocol.FileUploadSpec
	attachment *protocol.Attachment
	plugin     *protocol.PluginUploadSpec
	format     *protocol.PluginSkillsArchiveFormat
	download   bool
	size       uint64
	revision   string
	stamp      string
	cancel     context.CancelFunc
	service    link.Stream
}

// Transfers is the one resource owner for authenticated, single-use file streams.
// Prepared uploads retain confined publication targets until the durable finish.
type Transfers struct {
	mu          sync.Mutex
	profile     string
	entries     map[protocol.StreamID]*transferEntry
	closed      bool
	workers     sync.WaitGroup
	now         func() time.Time
	afterFunc   func(time.Duration, func()) func() bool
	withTimeout func(context.Context, time.Duration) (context.Context, context.CancelFunc)
}

func NewTransfers(profile string) *Transfers {
	return &Transfers{profile: profile, entries: map[protocol.StreamID]*transferEntry{}, now: time.Now, afterFunc: func(d time.Duration, f func()) func() bool { return time.AfterFunc(d, f).Stop }, withTimeout: context.WithTimeout}
}
func transferMissing() error {
	return &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "file transfer is unavailable"}
}
func transferUnavailable() error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "file transfer service is unavailable"}
}
func (t *Transfers) reserve(caller protocol.NodeID, root string, e *transferEntry) (protocol.StreamID, error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.closed {
		return protocol.StreamID{}, transferUnavailable()
	}
	id := protocol.NewStreamID()
	e.caller, e.root, e.state, e.expires = caller, root, "ready", t.now().Add(openTimeout)
	t.entries[id] = e
	t.expiryLocked(id, e)
	return id, nil
}
func (t *Transfers) expiryLocked(id protocol.StreamID, e *transferEntry) {
	if e.timer != nil {
		e.timer()
	}
	e.timer = t.afterFunc(openTimeout, func() {
		t.mu.Lock()
		if t.entries[id] == e && (e.state == "ready" || e.state == "uploaded") && !e.expires.After(t.now()) {
			delete(t.entries, id)
			t.mu.Unlock()
			cleanupTransfer(e)
			return
		}
		t.mu.Unlock()
	})
}
func cleanupTransfer(e *transferEntry) {
	if e.timer != nil {
		e.timer()
	}
	if e.cancel != nil {
		e.cancel()
	}
	if e.service != nil {
		_ = e.service.Close()
	}
	if e.file != nil {
		identity, err := e.file.Stat()
		if e.tempRoot != nil && err == nil {
			if current, readErr := e.tempRoot.Lstat(e.temporary); readErr == nil && os.SameFile(current, identity) {
				_ = e.tempRoot.Remove(e.temporary)
			}
		}
		_ = e.file.Close()
		if e.tempRoot != nil {
			if current, readErr := e.tempRoot.Lstat(e.temporary); readErr == nil && identity != nil && os.SameFile(current, identity) {
				_ = e.tempRoot.Remove(e.temporary)
			}
		}
	}
	if e.tempRoot != nil {
		_ = e.tempRoot.Close()
	}
	if e.target != nil {
		_ = e.target.Close()
	}
	if e.files != nil {
		_ = e.files.Close()
	}

}
func (t *Transfers) temporary() (*os.File, *os.Root, string, error) {
	if t.profile == "" {
		return nil, nil, "", transferUnavailable()
	}
	profile, err := os.OpenRoot(t.profile)
	if err != nil {
		return nil, nil, "", err
	}
	defer profile.Close()
	if err = profile.Mkdir("tmp", 0o700); err != nil && !errors.Is(err, os.ErrExist) {
		return nil, nil, "", err
	}
	identity, err := profile.Lstat("tmp")
	if err != nil {
		return nil, nil, "", err
	}
	if !identity.IsDir() || identity.Mode()&os.ModeSymlink != 0 {
		return nil, nil, "", invalid("temporary directory is not an ordinary directory")
	}
	root, err := profile.OpenRoot("tmp")
	if err != nil {
		return nil, nil, "", err
	}
	retained, err := root.Stat(".")
	if err != nil || !os.SameFile(identity, retained) {
		root.Close()
		return nil, nil, "", conflict("temporary directory changed")
	}
	name := "file-transfer-" + protocol.NewStreamID().String()
	file, err := root.OpenFile(name, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		root.Close()
		return nil, nil, "", err
	}
	return file, root, name, nil
}
func (t *Transfers) UploadFile(ctx context.Context, caller protocol.NodeID, root string, spec protocol.FileUploadSpec, access core.FileAccess) (protocol.FileUpload, error) {
	ctx, cancel := t.withTimeout(ctx, prepareTimeout)
	defer cancel()
	out := protocol.FileUpload{Spec: spec}
	if !validRevision(spec.Revision) || spec.ExpectedRevision != nil && !validRevision(*spec.ExpectedRevision) || spec.Size > math.MaxInt64 {
		return out, invalid("invalid upload content revision or size")
	}
	spec.Revision = strings.ToLower(spec.Revision)
	if spec.ExpectedRevision != nil {
		normalized := strings.ToLower(*spec.ExpectedRevision)
		spec.ExpectedRevision = &normalized
	}
	out.Spec = spec
	fs, err := core.OpenFiles(root, access)
	if err != nil {
		return out, err
	}
	e := &transferEntry{files: fs, upload: &spec, size: spec.Size, revision: spec.Revision}
	target, err := fs.PrepareUploadTarget(ctx, spec.Path, spec.ExpectedRevision, math.MaxUint64)
	if err != nil {
		cleanupTransfer(e)
		return out, err
	}
	e.target = target
	e.file, e.tempRoot, e.temporary, err = t.temporary()
	if err != nil {
		cleanupTransfer(e)
		return out, err
	}
	if err = ctx.Err(); err != nil {
		cleanupTransfer(e)
		return out, err
	}
	out.Stream, err = t.reserve(caller, root, e)
	if err != nil {
		cleanupTransfer(e)
	}
	return out, err
}
func (t *Transfers) UploadAttachment(ctx context.Context, caller protocol.NodeID, root string, spec protocol.AttachmentSpec) (protocol.AttachmentUpload, error) {
	ctx, cancel := t.withTimeout(ctx, prepareTimeout)
	defer cancel()
	out := protocol.AttachmentUpload{Spec: spec}
	if err := ValidateAttachment(spec); err != nil {
		return out, err
	}
	if !validRevision(spec.Revision) {
		return out, invalid("invalid attachment revision")
	}
	spec.Revision = strings.ToLower(spec.Revision)
	out.Spec = spec
	fs, err := t.attachmentDirectory(true)
	if err != nil {
		return out, err
	}
	attachment := protocol.Attachment{ID: protocol.NewAttachmentID(), Spec: spec}
	e := &transferEntry{files: fs, attachment: &attachment, size: spec.Size, revision: spec.Revision}
	e.target, err = fs.PrepareUploadTarget(ctx, attachment.ID.String(), nil, attachmentLimit)
	if err != nil {
		cleanupTransfer(e)
		return out, err
	}
	e.file, e.tempRoot, e.temporary, err = t.temporary()
	if err != nil {
		cleanupTransfer(e)
		return out, err
	}
	if err = ctx.Err(); err != nil {
		cleanupTransfer(e)
		return out, err
	}
	out.Stream, err = t.reserve(caller, root, e)
	if err != nil {
		cleanupTransfer(e)
	}
	return out, err
}
func (t *Transfers) DownloadFile(ctx context.Context, caller protocol.NodeID, root string, worktree protocol.WorktreeID, path string, access core.FileAccess) (protocol.FileDownload, error) {
	ctx, cancel := t.withTimeout(ctx, prepareTimeout)
	defer cancel()
	out := protocol.FileDownload{Worktree: worktree, Path: path}
	fs, err := core.OpenFiles(root, access)
	if err != nil {
		return out, err
	}
	defer fs.Close()
	stamp, err := fs.SourceRevision(path)
	if err != nil {
		return out, err
	}
	file, err := fs.Open(path)
	if err != nil {
		return out, err
	}
	e := &transferEntry{file: file, download: true, stamp: stamp}
	info, err := file.Stat()
	if err == nil {
		e.size = uint64(info.Size())
		e.revision, err = hashOpenFile(ctx, file, e.size)
	}
	if err == nil {
		err = fs.VerifySource(ctx, path, e.revision, stamp)
	}
	if err != nil {
		cleanupTransfer(e)
		return out, err
	}
	out.Size, out.Revision, out.Stamp = e.size, e.revision, stamp
	out.Stream, err = t.reserve(caller, root, e)
	if err != nil {
		cleanupTransfer(e)
	}
	return out, err
}
func hashOpenFile(ctx context.Context, file *os.File, size uint64) (string, error) {
	hash := blake3.New(32, nil)
	buffer := make([]byte, protocol.FILE_TRANSFER_CHUNK_BYTES)
	var total uint64
	for {
		if err := ctx.Err(); err != nil {
			return "", err
		}
		n, err := file.Read(buffer)
		if n > 0 {
			if uint64(n) > size-total {
				return "", conflict("download source changed")
			}
			total += uint64(n)
			_, _ = hash.Write(buffer[:n])
		}
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return "", err
		}
	}
	if total != size {
		return "", conflict("download source changed")
	}
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return "", err
	}
	return fmtRevision(hash.Sum(nil)), nil
}
func fmtRevision(data []byte) string {
	const digits = "0123456789abcdef"
	out := make([]byte, len(data)*2)
	for i, b := range data {
		out[2*i] = digits[b>>4]
		out[2*i+1] = digits[b&15]
	}
	return string(out)
}
func (t *Transfers) DownloadAttachment(ctx context.Context, caller protocol.NodeID, root string, attachment protocol.Attachment) (protocol.AttachmentDownload, error) {
	out := protocol.AttachmentDownload{Attachment: attachment}
	file, err := t.OpenAttachment(attachment)
	if err != nil {
		return out, err
	}
	e := &transferEntry{file: file, download: true, attachment: &attachment, size: attachment.Spec.Size, revision: attachment.Spec.Revision}
	if err = ctx.Err(); err != nil {
		cleanupTransfer(e)
		return out, err
	}
	out.Stream, err = t.reserve(caller, root, e)
	if err != nil {
		cleanupTransfer(e)
	}
	return out, err
}
func (t *Transfers) Open(ctx context.Context, caller protocol.NodeID, id protocol.StreamID) (link.Stream, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	t.mu.Lock()
	e := t.entries[id]
	if e == nil || e.caller != caller {
		t.mu.Unlock()
		return nil, transferMissing()
	}
	if e.state != "ready" {
		t.mu.Unlock()
		return nil, transferMissing()
	}
	if !e.expires.After(t.now()) {
		delete(t.entries, id)
		t.mu.Unlock()
		cleanupTransfer(e)
		return nil, transferMissing()
	}
	client, service := link.StreamPair()
	workerContext, cancel := context.WithCancel(context.Background())
	e.state, e.service, e.cancel = "active", service, cancel
	if e.timer != nil {
		e.timer()
	}
	t.workers.Add(1)
	t.mu.Unlock()
	go t.run(workerContext, id, e)
	return client, nil
}
func (t *Transfers) transferStep(ctx context.Context, stream link.Stream, op func() error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	var expired atomic.Bool
	done := make(chan struct{})
	timer := t.afterFunc(idleTimeout, func() { defer close(done); expired.Store(true); _ = stream.Close() })
	err := op()
	if !timer() {
		<-done
	}
	if ctx.Err() != nil {
		return ctx.Err()
	}
	if expired.Load() {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "file transfer made no progress"}
	}
	return err
}
func (t *Transfers) run(ctx context.Context, id protocol.StreamID, e *transferEntry) {
	defer t.workers.Done()
	cancellationDone := make(chan struct{})
	stop := context.AfterFunc(ctx, func() { defer close(cancellationDone); _ = e.service.Close() })
	defer func() {
		if !stop() {
			<-cancellationDone
		}
	}()
	uploaded := false
	defer func() {
		_ = e.service.Close()
		if !uploaded {
			t.mu.Lock()
			if t.entries[id] == e {
				delete(t.entries, id)
			}
			t.mu.Unlock()
			cleanupTransfer(e)
		}
	}()
	buffer := make([]byte, protocol.FILE_TRANSFER_CHUNK_BYTES)
	var total uint64
	hash := blake3.New(32, nil)
	for {
		var n int
		var err error
		if e.download {
			remaining := e.size - total
			if remaining == 0 {
				break
			}
			err = t.transferStep(ctx, e.service, func() error {
				var er error
				n, er = e.file.Read(buffer[:min(uint64(len(buffer)), remaining)])
				return er
			})
		} else {
			err = t.transferStep(ctx, e.service, func() error { var er error; n, er = e.service.Read(buffer); return er })
		}
		if err != nil && !errors.Is(err, io.EOF) {
			return
		}
		if n > 0 {
			if uint64(n) > e.size-total {
				return
			}
			total += uint64(n)
			if e.download {
				if er := t.transferStep(ctx, e.service, func() error { return writeBytes(e.service, buffer[:n]) }); er != nil {
					return
				}
			} else {
				if er := t.transferStep(ctx, e.service, func() error { return writeBytes(e.file, buffer[:n]) }); er != nil {
					return
				}
				_, _ = hash.Write(buffer[:n])
			}
		}
		if errors.Is(err, io.EOF) {
			break
		}
	}
	if total != e.size {
		return
	}
	if e.download {
		_ = t.transferStep(ctx, e.service, e.service.CloseWrite)
		return
	}
	if fmtRevision(hash.Sum(nil)) != e.revision {
		return
	}
	if err := t.transferStep(ctx, e.service, e.file.Sync); err != nil {
		return
	}
	if _, err := e.file.Seek(0, io.SeekStart); err != nil {
		return
	}
	t.mu.Lock()
	if t.entries[id] != e || ctx.Err() != nil {
		t.mu.Unlock()
		return
	}
	e.state, e.expires = "uploaded", t.now().Add(openTimeout)
	t.expiryLocked(id, e)
	uploaded = true
	t.mu.Unlock()
	if err := t.transferStep(ctx, e.service, func() error { return writeBytes(e.service, []byte{protocol.FILE_UPLOAD_READY}) }); err != nil {
		return
	}
	_ = t.transferStep(ctx, e.service, e.service.CloseWrite)
}
func writeBytes(w io.Writer, data []byte) error {
	for len(data) > 0 {
		n, err := w.Write(data)
		if err != nil {
			return err
		}
		if n == 0 {
			return io.ErrShortWrite
		}
		data = data[n:]
	}
	return nil
}
func (t *Transfers) Finish(ctx context.Context, caller protocol.NodeID, root string, command protocol.Command) (protocol.Output, error) {
	var id protocol.StreamID
	var worktree protocol.WorktreeID
	path := ""
	switch command.Kind {
	case "finish_file_upload":
		v := command.Data.(*protocol.CommandFinishFileUploadPayload)
		id, worktree, path = v.Stream, v.Worktree, v.Path
	case "finish_attachment_upload":
		v := command.Data.(*protocol.CommandFinishAttachmentUploadPayload)
		id, worktree = v.Stream, v.Worktree
	default:
		return protocol.Output{}, invalid("invalid transfer finish command")
	}
	t.mu.Lock()
	e := t.entries[id]
	if e == nil || e.caller != caller || e.root != root || e.upload != nil && (e.upload.Worktree != worktree || e.upload.Path != path) || e.attachment != nil && e.attachment.Spec.Worktree != worktree {
		t.mu.Unlock()
		return protocol.Output{}, transferMissing()
	}
	if e.state != "uploaded" {
		t.mu.Unlock()
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "upload is not ready for publication"}
	}
	if !e.expires.After(t.now()) {
		delete(t.entries, id)
		t.mu.Unlock()
		cleanupTransfer(e)
		return protocol.Output{}, transferMissing()
	}
	e.state = "finishing"
	if e.timer != nil {
		e.timer()
	}
	t.workers.Add(1)
	t.mu.Unlock()
	defer t.workers.Done()
	defer func() { t.mu.Lock(); delete(t.entries, id); t.mu.Unlock(); cleanupTransfer(e) }()
	if err := ctx.Err(); err != nil {
		return protocol.Output{}, err
	}
	if e.target == nil {
		return protocol.Output{}, transferMissing()
	}
	target := e.target
	e.target = nil
	written, err := target.Write(e.file, e.size, e.revision)
	if err != nil {
		return protocol.Output{}, err
	}
	if e.attachment != nil {
		return protocol.Output{Kind: "attachment", Data: *e.attachment}, nil
	}
	return protocol.Output{Kind: "file_written", Data: written}, nil
}
func (t *Transfers) Cancel(caller protocol.NodeID, id protocol.StreamID) error {
	t.mu.Lock()
	e := t.entries[id]
	if e == nil {
		t.mu.Unlock()
		return nil
	}
	if e.caller != caller {
		t.mu.Unlock()
		return transferMissing()
	}
	if e.state == "finishing" {
		t.mu.Unlock()
		return &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "upload publication has started; inspect the command result"}
	}
	delete(t.entries, id)
	active := e.state == "active"
	if active && e.cancel != nil {
		e.cancel()
	}
	t.mu.Unlock()
	if !active {
		cleanupTransfer(e)
	}
	return nil
}
func (t *Transfers) Remove(root string) {
	t.mu.Lock()
	var removed []*transferEntry
	for id, e := range t.entries {
		relative, err := filepath.Rel(root, e.root)
		if err == nil && (relative == "." || relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator))) {
			if e.state == "finishing" {
				continue
			}
			delete(t.entries, id)
			if e.state == "active" {
				e.cancel()
			} else {
				removed = append(removed, e)
			}
		}
	}
	t.mu.Unlock()
	for _, e := range removed {
		cleanupTransfer(e)
	}
}
func (t *Transfers) Close() error {
	t.mu.Lock()
	t.closed = true
	var removed []*transferEntry
	for id, e := range t.entries {
		if e.state == "finishing" {
			continue
		}
		delete(t.entries, id)
		if e.state == "active" {
			e.cancel()
		} else {
			removed = append(removed, e)
		}
	}
	t.mu.Unlock()
	for _, e := range removed {
		cleanupTransfer(e)
	}
	t.workers.Wait()
	return nil
}

// DownloadBytes serves a captured canonical image without exposing blob storage.
func (t *Transfers) DownloadBytes(ctx context.Context, caller protocol.NodeID, root string, attachment protocol.Attachment, data []byte) (protocol.AttachmentDownload, error) {
	out := protocol.AttachmentDownload{Attachment: attachment}
	if uint64(len(data)) != attachment.Spec.Size || revision(data) != attachment.Spec.Revision {
		return out, conflict("image content changed")
	}
	entry := &transferEntry{download: true, attachment: &attachment, size: attachment.Spec.Size, revision: attachment.Spec.Revision}
	var err error
	entry.file, entry.tempRoot, entry.temporary, err = t.temporary()
	if err != nil {
		return out, err
	}
	if err = ctx.Err(); err == nil {
		err = writeBytes(entry.file, data)
	}
	if err == nil {
		_, err = entry.file.Seek(0, io.SeekStart)
	}
	if err != nil {
		cleanupTransfer(entry)
		return out, err
	}
	out.Stream, err = t.reserve(caller, root, entry)
	if err != nil {
		cleanupTransfer(entry)
	}
	return out, err
}
