package live

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceUploadTransport struct {
	mu         sync.Mutex
	node       protocol.NodeID
	conflict   bool
	uncertain  bool
	specs      map[protocol.StreamID]protocol.FileUploadSpec
	bytes      map[protocol.StreamID][]byte
	outcomes   map[protocol.RequestID]protocol.Output
	requests   []protocol.Request
	finishGate chan struct{}
}

func (f *workspaceUploadTransport) Target() protocol.NodeID { return f.node }
func (f *workspaceUploadTransport) WatchConnection(ctx context.Context, changed func(link.ConnectionInfo)) error {
	return link.NewLocal(f.Target(), f.Target(), nil).WatchConnection(ctx, changed)
}
func (*workspaceUploadTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return nil, errors.New("upload fixture has no subscriptions")
}
func (f *workspaceUploadTransport) Open(_ context.Context, id protocol.StreamID) (link.Stream, error) {
	controller, server := link.StreamPair()
	go func() {
		defer server.Close()
		body, err := io.ReadAll(server)
		if err == nil {
			f.mu.Lock()
			f.bytes[id] = body
			f.mu.Unlock()
			_, err = server.Write([]byte{protocol.FILE_UPLOAD_READY})
			if err == nil {
				_ = server.CloseWrite()
			}
		}
	}()
	return controller, nil
}
func (f *workspaceUploadTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.requests = append(f.requests, request)
	response := link.Response{}
	switch request.Command.Kind {
	case "upload_file":
		spec, _ := detail[protocol.FileUploadSpec](request.Command.Data)
		if f.conflict && spec.ExpectedRevision == nil {
			response.Fault = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "destination exists"}
		} else {
			stream := protocol.NewStreamID()
			f.specs[stream] = spec
			response.Output = protocol.Output{Kind: "file_upload", Data: protocol.FileUpload{Stream: stream, Spec: spec}}
		}
	case "download_file":
		input, _ := detail[protocol.CommandDownloadFilePayload](request.Command.Data)
		response.Output = protocol.Output{Kind: "file_download", Data: protocol.FileDownload{Worktree: input.Worktree, Path: input.Path, Stream: protocol.NewStreamID(), Revision: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}
	case "finish_file_upload":
		input, _ := detail[protocol.CommandFinishFileUploadPayload](request.Command.Data)
		spec := f.specs[input.Stream]
		response.Output = protocol.Output{Kind: "file_written", Data: protocol.FileWritten{Path: spec.Path, Revision: spec.Revision, Size: spec.Size}}
		f.outcomes[request.ID] = response.Output
		if f.uncertain {
			response.Fault = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "publication observation lost"}
		}
	case "inspect_request":
		input, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
		output := f.outcomes[input.ID]
		response.Output = protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: input.ID, Outcome: protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}}}}
	case "cancel_file_transfer":
		response.Output = protocol.Output{Kind: "file_transfer_cancelled"}
	default:
		response.Fault = &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected fixture command"}
	}
	completion := make(chan link.Response, 1)
	if request.Command.Kind == "finish_file_upload" && f.finishGate != nil {
		go func() {
			select {
			case <-f.finishGate:
				completion <- response
			case <-ctx.Done():
			}
		}()
	} else {
		completion <- response
	}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}, nil
}

// testWorkspaceUploads is grouped under the existing controller IO test.
func testWorkspaceUploads(t *testing.T) {
	for _, mode := range []string{"queue", "replace", "uncertain", "cancel staging", "cancel remaining"} {
		t.Run(mode, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			transport := &workspaceUploadTransport{node: protocol.NodeID{1}, conflict: mode == "replace", uncertain: mode == "uncertain", specs: map[protocol.StreamID]protocol.FileUploadSpec{}, bytes: map[protocol.StreamID][]byte{}, outcomes: map[protocol.RequestID]protocol.Output{}}
			if mode == "cancel remaining" {
				transport.finishGate = make(chan struct{})
			}
			r := &Runtime{ctx: ctx, controller: client.New(transport)}
			updates := make(chan struct{}, 1)
			r.invalidate = func() {
				select {
				case updates <- struct{}{}:
				default:
				}
			}
			defer func() { cancel(); r.workers.Wait() }()
			state := &workspaceUploads{ctx: ctx, cancel: cancel, items: map[protocol.RequestID]*workspaceUpload{}}
			worktree := protocol.NewWorktreeID()
			p := &workspaceProject{runtime: r, owner: protocol.Worktree{ID: worktree}}
			p.panel = panel.NewLiveProject("fixture", &viewmodel.Workspace{Files: &viewmodel.FileNode{}}, &panel.Actions{})
			r.workspace = &workspaceState{uploads: state, projects: map[protocol.WorktreeID]*workspaceProject{worktree: p}}
			payload := []byte{'b', 0, 0xff, 'y', 't', 'e', 's'}
			source := filepath.Join(t.TempDir(), "source.bin")
			if err := os.WriteFile(source, payload, 0o600); err != nil {
				t.Fatal(err)
			}
			upload := &workspaceUpload{owner: state, project: p, id: protocol.NewRequestID(), worktree: worktree, directory: "captured", paths: []string{source}}
			if mode == "queue" || mode == "cancel remaining" {
				upload.paths = append(upload.paths, filepath.Join(t.TempDir(), "second.bin"))
				if err := os.WriteFile(upload.paths[1], payload, 0o600); err != nil {
					t.Fatal(err)
				}
			}
			state.current, state.items[upload.id] = upload, upload
			upload.prepare(true)
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				r.mu.Lock()
				queue := r.queue
				r.queue = nil
				r.mu.Unlock()
				for _, complete := range queue {
					complete(c)
				}
			}, 600, 400)
			pump := func(done func() bool) {
				t.Helper()
				for !done() {
					tester.Frame()
					if done() {
						return
					}
					select {
					case <-updates:
					case <-ctx.Done():
						t.Fatalf("upload cancelled before settling: status=%d phase=%s index=%d", upload.view.Status, upload.view.Phase, upload.index)
					}
				}
			}
			upload.start(false)
			if mode == "cancel staging" {
				upload.close()
			}
			if mode == "replace" {
				pump(func() bool { return upload.view.Status == transfers.EntryExists })
				if upload.publication != nil || upload.index != 0 || upload.existing == nil {
					t.Fatal("destination conflict published or advanced the queue")
				}
				upload.start(true)
			}
			if mode == "uncertain" {
				pump(func() bool { return upload.view.Status == transfers.EntryUncertain })
				if upload.publication == nil || upload.index != 0 || len(p.pendingWorkspaceUploads()) != 1 {
					t.Fatal("uncertain publication lost its identity or advanced the queue")
				}
				retained := upload.publication.ID
				upload.check()
				pump(func() bool { return upload.view.Status == transfers.EntryDone })
				transport.mu.Lock()
				finishes, inspections := 0, 0
				for _, request := range transport.requests {
					if request.Command.Kind == "finish_file_upload" {
						finishes++
						if request.ID != retained {
							t.Error("publication request identity changed")
						}
					}
					if request.Command.Kind == "inspect_request" {
						inspections++
					}
				}
				transport.mu.Unlock()
				if finishes != 1 || inspections != 1 {
					t.Fatal("outcome inspection replayed staged IO", finishes, inspections)
				}
			} else if mode == "cancel remaining" {
				// A close while the exact publication is pending cannot cancel it,
				// but prevents admission of the next selected source.
				pump(func() bool { return upload.publication != nil || upload.index > 0 })
				upload.close()
				close(transport.finishGate)
				pump(func() bool { return upload.view.Status != transfers.EntryRunning })
				if upload.index != 1 {
					t.Fatal("closing admitted the remaining source", upload.index)
				}
			} else if mode == "cancel staging" {
				pump(func() bool { return upload.view.Status != transfers.EntryRunning })
				if upload.index != 0 || upload.publication != nil || upload.view.Status != transfers.EntryCancelled {
					t.Fatal("cancelled staging published or advanced the queue")
				}
				transport.mu.Lock()
				for _, request := range transport.requests {
					if request.Command.Kind == "finish_file_upload" {
						t.Error("cancelled staged bytes were published")
					}
				}
				transport.mu.Unlock()
			} else {
				pump(func() bool { return upload.view.Status == transfers.EntryDone })
			}
			cancel()
			r.workers.Wait()
			transport.mu.Lock()
			for stream, body := range transport.bytes {
				if !bytes.Equal(body, payload) || transport.specs[stream].Worktree != worktree {
					t.Error("verified bytes or captured worktree changed", stream)
				}
				if mode == "replace" && (transport.specs[stream].ExpectedRevision == nil || *transport.specs[stream].ExpectedRevision != "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa") {
					t.Error("replacement lost its observed destination revision")
				}
			}
			transport.mu.Unlock()
		})
	}
}
