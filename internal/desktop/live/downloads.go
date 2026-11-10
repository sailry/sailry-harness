package live

import (
	"context"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) downloadOutcome(c *ui.Context, err error) {
	r.reportKeys(c, err, "files_download_failed", "files_download_conflict", "workspace_action_unknown")
}
func (r *Runtime) downloadFileCard(x *conversationActions, file conversation.File, external bool) {
	descriptor, ok := r.attachmentDescriptor(x.cv.ID, file.ID)
	if !ok {
		r.report(x.c, fileActionFault(protocol.ErrorCodeNotFound, "file source is unavailable"), "workspace_changed")
		return
	}
	descriptor.name = file.Name()
	if descriptor.artifact != nil {
		if external {
			r.openExternalArtifact(descriptor)
		} else {
			r.chooseArtifactDestination(descriptor)
		}
		return
	}
	if external {
		r.openExternalAttachment(descriptor, false)
	} else {
		r.chooseAttachmentDestination(descriptor)
	}
}
func (r *Runtime) chooseAttachmentDestination(descriptor attachmentDescriptor) {
	if r.attachments == nil {
		return
	}
	native := r.attachments.native
	if native.SaveFile == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "native save panel is unavailable"))
		return
	}
	native.SaveFile(filepath.Base(descriptor.name), func(path string) {
		if path == "" {
			return
		}
		r.enqueue(func(*ui.Context) { r.beginAttachmentDownload(descriptor, path, true, nil) })
	})
}
func (r *Runtime) beginAttachmentDownload(descriptor attachmentDescriptor, path string, dialog bool, finished func(string, error)) {
	state := r.attachments
	if state == nil {
		return
	}
	if dialog && r.transferModalBusy(false) {
		r.Report(fileActionFault(protocol.ErrorCodeBusy, "another file transfer dialog is open"))
		return
	}
	if dialog && state.download != nil && state.download.Status != transfers.Finished {
		r.Report(fileActionFault(protocol.ErrorCodeBusy, "another download is still active"))
		return
	}
	ctx, cancel := context.WithCancel(state.ctx)
	generation := uint64(0)
	if dialog {
		state.downloadGeneration++
		generation = state.downloadGeneration
		state.downloadCancel = cancel
		state.download = &transfers.Download{Open: true, Name: filepath.Base(descriptor.name), Destination: path, Status: transfers.Preparing}
	}
	r.async(func() {
		defer cancel()
		target, err := prepareDownloadDestination(path)
		if err == nil {
			defer target.close()
			var stream protocol.StreamID
			var size uint64
			var revision string
			stream, size, revision, err = r.prepareAttachmentDownload(ctx, descriptor)
			if err == nil {
				last := time.Time{}
				err = r.controller.Download(ctx, stream, size, revision, target.file, func(copied uint64) {
					now := time.Now()
					if copied != 0 && copied != size && !last.IsZero() && now.Sub(last) < 75*time.Millisecond {
						return
					}
					last = now
					if dialog {
						r.enqueue(func(*ui.Context) {
							if state.downloadGeneration == generation {
								state.download.Status, state.download.Copied, state.download.Size = transfers.Receiving, int64(copied), int64(size)
							}
						})
					}
				})
				if err == nil {
					if dialog {
						r.enqueue(func(*ui.Context) {
							if state.downloadGeneration == generation {
								state.download.Status = transfers.Publishing
							}
						})
					}
					err = target.publish(ctx)
				}
			}
		}
		result := err
		r.enqueue(func(c *ui.Context) {
			if dialog && state.downloadGeneration == generation {
				state.download.Status = transfers.Finished
				state.downloadCancel = nil
				state.download.Error = ""
				if result != nil {
					state.download.Error = "files_download_failed"
					if errors.Is(result, context.Canceled) || faultCode(result) == protocol.ErrorCodeCancelled {
						state.download.Error = "files_download_cancelled"
					} else if faultCode(result) == protocol.ErrorCodeRevisionConflict || faultCode(result) == protocol.ErrorCodeConflict {
						state.download.Error = "files_download_conflict"
					}
				}
			}
			if result != nil {
				if errors.Is(result, context.Canceled) || faultCode(result) == protocol.ErrorCodeCancelled {
					kit.Notify(c, "", i18n.T(c, "files_download_cancelled"))
				} else {
					r.downloadOutcome(c, result)
				}
			} else if dialog {
				kit.Toast(c, kit.Done, i18n.T(c, "files_download_done"))
			}
			if finished != nil {
				finished(path, result)
			}
		})
	})
}

func (r *Runtime) attachmentDownloadOpen() bool {
	return r.attachments != nil && r.attachments.download != nil && r.attachments.download.Open
}
func (r *Runtime) ownedDownloadDirectory() (string, func(), error) {
	directory, err := os.MkdirTemp("", "sailry-open-")
	if err != nil {
		return "", nil, err
	}
	identity, err := os.Lstat(directory)
	if err != nil {
		_ = os.Remove(directory)
		return "", nil, err
	}
	var once sync.Once
	release := func() {
		once.Do(func() {
			if current, e := os.Lstat(directory); e == nil && os.SameFile(identity, current) {
				_ = os.RemoveAll(directory)
			}
		})
	}
	state := r.attachments
	state.mu.Lock()
	if state.closed {
		state.mu.Unlock()
		release()
		return "", nil, context.Canceled
	}
	state.temporary[directory] = release
	state.mu.Unlock()
	return directory, func() { state.mu.Lock(); delete(state.temporary, directory); state.mu.Unlock(); release() }, nil
}
func (r *Runtime) openExternalAttachment(descriptor attachmentDescriptor, reveal bool) {
	if r.attachments == nil {
		return
	}
	native := r.attachments.native
	if !reveal && native.OpenLocal == nil || reveal && native.RevealLocal == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "native file action is unavailable"))
		return
	}
	r.async(func() {
		directory, release, err := r.ownedDownloadDirectory()
		if err != nil {
			r.Report(err)
			return
		}
		path := filepath.Join(directory, filepath.Base(descriptor.name))
		r.enqueue(func(*ui.Context) {
			r.beginAttachmentDownload(descriptor, path, false, func(path string, err error) {
				if err != nil {
					release()
					return
				}
				if reveal {
					native.RevealLocal(path)
				} else {
					native.OpenLocal(path)
				}
			})
		})
	})
}
func (r *Runtime) localImageAction(a *composer.Attachment, action kit.ImageAction) {
	native := r.attachments.native
	data := append([]byte(nil), a.Data...)
	write := func(path string, finished func(error)) {
		r.async(func() {
			target, err := prepareDownloadDestination(path)
			if err == nil {
				defer target.close()
				_, err = target.file.Write(data)
				if err == nil {
					err = target.publish(r.attachments.ctx)
				}
			}
			r.enqueue(func(c *ui.Context) {
				if err != nil {
					r.downloadOutcome(c, err)
				}
				if finished != nil {
					finished(err)
				} else if err == nil {
					kit.Toast(c, kit.Done, i18n.T(c, "files_download_done"))
				}
			})
		})
	}
	switch action {
	case kit.ImageSave:
		if native.SaveFile == nil {
			r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "native save panel is unavailable"))
			return
		}
		native.SaveFile(a.Name, func(path string) {
			if path != "" {
				write(path, nil)
			}
		})
	case kit.ImageOpen:
		if native.OpenLocal == nil {
			r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "native file action is unavailable"))
			return
		}
		r.async(func() {
			directory, release, err := r.ownedDownloadDirectory()
			if err != nil {
				r.Report(err)
				return
			}
			r.enqueue(func(*ui.Context) {
				path := filepath.Join(directory, filepath.Base(a.Name))
				write(path, func(err error) {
					if err != nil {
						release()
						return
					}
					native.OpenLocal(path)
				})
			})
		})
	}
}
func (r *Runtime) OpenWorkspaceExternal(worktree protocol.WorktreeID, path string, expected *string) {
	expected = cloneAttachmentRevision(expected)
	r.enqueue(func(*ui.Context) {
		r.openExternalAttachment(attachmentDescriptor{worktree: worktree, path: path, name: filepath.Base(path), revision: expected}, false)
	})
}
func (r *Runtime) DownloadWorkspaceFile(worktree protocol.WorktreeID, path string, expected *string) {
	expected = cloneAttachmentRevision(expected)
	r.enqueue(func(*ui.Context) {
		r.chooseAttachmentDestination(attachmentDescriptor{worktree: worktree, path: path, name: filepath.Base(path), revision: expected})
	})
}
func (r *Runtime) RevealWorkspaceFile(worktree protocol.WorktreeID, path string) {
	r.enqueue(func(c *ui.Context) {
		if r.attachments == nil || r.attachments.native.RevealLocal == nil {
			r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeUnavailable, "native reveal is unavailable"))
			return
		}
		if r.attachments.native.LocalNode != r.controller.Target() {
			r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeUnavailable, "the execution host cannot be revealed on this controller"))
			return
		}
		for _, value := range r.controller.Snapshot().Worktrees {
			if value.ID == worktree {
				if !validWorkspaceRelative(path) {
					r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid workspace file path"))
					return
				}
				r.attachments.native.RevealLocal(filepath.Join(value.Path, filepath.FromSlash(path)))
				return
			}
		}
		r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeNotFound, "worktree is unavailable"))
	})
}

func validWorkspaceRelative(path string) bool {
	return fs.ValidPath(path) && path != "." && !strings.ContainsAny(path, "\\:")
}
func (r *Runtime) InsertWorkspaceReferences(worktree protocol.WorktreeID, paths []string) {
	paths = slices.Clone(paths)
	r.enqueue(func(c *ui.Context) {
		selected := r.App.Sidebar.Selected
		session, ok := r.session(selected)
		if !ok || session.Worktree != worktree {
			r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeWrongTarget, "file references belong to another workspace"))
			return
		}
		for _, path := range paths {
			if !validWorkspaceRelative(path) {
				r.downloadOutcome(c, fileActionFault(protocol.ErrorCodeInvalidRequest, "invalid file reference"))
				return
			}
		}
		r.App.Composer(selected).InsertFiles(paths, "")
	})
}
