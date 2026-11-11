package client

import (
	"bytes"
	"context"
	"encoding/hex"
	"io"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type transferBuffer struct{ bytes.Buffer }

func (*transferBuffer) Close() error { return nil }

// liveTransport refuses a cancelled caller as a connected route does.
type liveTransport struct{ *clientTransport }

func (transport liveTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if err := ctx.Err(); err != nil {
		return link.Admission{}, err
	}
	return transport.clientTransport.Dispatch(ctx, request)
}

func TestTransferStream(t *testing.T) {
	t.Parallel()
	payload := []byte("verified transfer bytes")
	hash := blake3.Sum256(payload)
	revision := hex.EncodeToString(hash[:])
	for _, direction := range []string{"upload", "download"} {
		for _, changed := range []bool{false, true} {
			name := direction
			if changed {
				name += "_changed"
			}
			t.Run(name, func(t *testing.T) {
				local, server := link.StreamPair()
				defer local.Close()
				defer server.Close()
				id := protocol.NewStreamID()
				transport := &clientTransport{open: func(_ context.Context, actual protocol.StreamID) (link.Stream, error) {
					if actual != id {
						t.Error("stream identity changed")
					}
					return local, nil
				}}
				controller := New(transport)
				expected := revision
				if changed {
					expected = hex.EncodeToString(make([]byte, 32))
				}
				peer := make(chan error, 1)
				go func() {
					defer server.Close()
					if direction == "upload" {
						data, err := io.ReadAll(server)
						if err == nil && !bytes.Equal(data, payload) {
							err = io.ErrUnexpectedEOF
						}
						if err == nil && !changed {
							_, err = server.Write([]byte{protocol.FILE_UPLOAD_READY})
							if err == nil {
								err = server.CloseWrite()
							}
						}
						peer <- err
					} else {
						var empty [1]byte
						n, err := server.Read(empty[:])
						if n != 0 || err != io.EOF {
							peer <- io.ErrUnexpectedEOF
							return
						}
						_, err = server.Write(payload)
						if err == nil {
							err = server.CloseWrite()
						}
						peer <- err
					}
				}()
				var progress []uint64
				update := func(n uint64) { progress = append(progress, n) }
				var err error
				if direction == "upload" {
					err = controller.Upload(t.Context(), id, uint64(len(payload)), expected, io.NopCloser(bytes.NewReader(payload)), update)
				} else {
					out := &transferBuffer{}
					err = controller.Download(t.Context(), id, uint64(len(payload)), expected, out, update)
					if !bytes.Equal(out.Bytes(), payload) {
						t.Fatal("download changed bytes")
					}
				}
				if changed {
					if faultCode(err) != protocol.ErrorCodeRevisionConflict {
						t.Fatalf("changed transfer = %v", err)
					}
				} else if err != nil {
					t.Fatal(err)
				}
				if err := <-peer; err != nil {
					t.Fatal(err)
				}
				if len(progress) < 2 || progress[0] != 0 || progress[len(progress)-1] != uint64(len(payload)) {
					t.Fatalf("progress = %v", progress)
				}
				transport.mu.Lock()
				requests := append([]protocol.Request(nil), transport.requests...)
				transport.mu.Unlock()
				if changed {
					if len(requests) != 1 || requests[0].Command.Kind != "cancel_file_transfer" {
						t.Fatalf("failed transfer cleanup = %v", requests)
					}
				} else if len(requests) != 0 {
					t.Fatal("stream unexpectedly dispatched a publication command")
				}
			})
		}
	}
	t.Run("stalled", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			local, server := link.StreamPair()
			defer local.Close()
			defer server.Close()
			transport := &clientTransport{open: func(context.Context, protocol.StreamID) (link.Stream, error) { return local, nil }}
			transport.dispatch = func(actual protocol.Request) (link.Admission, error) {
				return link.Admission{Receipt: protocol.Receipt{ID: actual.ID}, Completion: make(chan link.Response)}, nil
			}
			started := time.Now()
			err := New(transport).Download(t.Context(), protocol.NewStreamID(), uint64(len(payload)), revision, &transferBuffer{}, nil)
			// Thirty seconds without progress, then two for the unanswered cleanup.
			if faultCode(err) != protocol.ErrorCodeUnavailable || time.Since(started) != 32*time.Second {
				t.Fatalf("stalled download = %v after %s", err, time.Since(started))
			}
		})
	})
	t.Run("port_open_cancelled", func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		id := protocol.NewStreamID()
		transport := &clientTransport{open: func(context.Context, protocol.StreamID) (link.Stream, error) {
			cancel()
			return nil, context.Canceled
		}}
		transport.dispatch = func(actual protocol.Request) (link.Admission, error) {
			completion := make(chan link.Response, 1)
			completion <- link.Response{Output: protocol.Output{Kind: "port_stream", Data: protocol.OutputPortStreamPayload{Stream: id}}}
			return link.Admission{Receipt: protocol.Receipt{ID: actual.ID}, Completion: completion}, nil
		}
		if _, err := New(liveTransport{transport}).portStream(ctx, 8080); err == nil {
			t.Fatal("a failed open returned a port stream")
		}
		transport.mu.Lock()
		requests := append([]protocol.Request(nil), transport.requests...)
		transport.mu.Unlock()
		released, ok := requests[len(requests)-1].Command.Data.(protocol.CommandCancelPortPayload)
		if len(requests) != 2 || requests[1].Command.Kind != "cancel_port" || !ok || released.Stream != id {
			t.Fatalf("cancelled open left its reserved stream: %v", requests)
		}
	})
	t.Run("cancelled", func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		transport := &clientTransport{open: func(context.Context, protocol.StreamID) (link.Stream, error) {
			t.Fatal("cancelled upload opened resource")
			return nil, nil
		}}
		controller := New(transport)
		err := controller.Upload(ctx, protocol.NewStreamID(), 0, revision, io.NopCloser(bytes.NewReader(nil)), nil)
		if faultCode(err) != protocol.ErrorCodeCancelled {
			t.Fatalf("cancelled upload = %v", err)
		}
	})
}
