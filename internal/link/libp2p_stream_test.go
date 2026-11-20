package link

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net"
	"testing"
	"testing/synctest"
	"time"

	"github.com/libp2p/go-libp2p/core/network"
)

type libp2pPipeStream struct {
	network.Stream
	reader, writer net.Conn
	readErr        error
}

func libp2pPipePair(t *testing.T) (*libp2pPipeStream, *libp2pPipeStream) {
	t.Helper()
	firstIn, secondOut := net.Pipe()
	secondIn, firstOut := net.Pipe()
	first := &libp2pPipeStream{reader: firstIn, writer: firstOut}
	second := &libp2pPipeStream{reader: secondIn, writer: secondOut}
	t.Cleanup(func() { _ = first.Close(); _ = second.Close() })
	return first, second
}

func (stream *libp2pPipeStream) Read(data []byte) (int, error) {
	if stream.readErr != nil {
		return 0, stream.readErr
	}
	return stream.reader.Read(data)
}

func (stream *libp2pPipeStream) Write(data []byte) (int, error) { return stream.writer.Write(data) }
func (stream *libp2pPipeStream) CloseWrite() error              { return stream.writer.Close() }
func (stream *libp2pPipeStream) CloseRead() error               { return stream.reader.Close() }
func (stream *libp2pPipeStream) Close() error {
	return errors.Join(stream.CloseRead(), stream.CloseWrite())
}
func (stream *libp2pPipeStream) Reset() error { return stream.Close() }
func (stream *libp2pPipeStream) SetWriteDeadline(at time.Time) error {
	return stream.writer.SetWriteDeadline(at)
}

func libp2pTestPair(t *testing.T) (*libp2pStream, *libp2pStream) {
	t.Helper()
	left, right := libp2pPipePair(t)
	client, host := newLibp2pStream(left, nil, nil), newLibp2pStream(right, nil, nil)
	t.Cleanup(func() { _ = client.Close(); _ = host.Close() })
	return client, host
}

func TestLibp2pStreamHalfClosePreservesResponse(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		client, host := libp2pTestPair(t)
		upload := bytes.Repeat([]byte("file payload"), libp2pRecordLimit)
		sent := make(chan error, 1)
		go func() {
			if _, err := client.Write(upload); err != nil {
				sent <- err
				return
			}
			sent <- client.CloseWrite()
		}()
		if received, err := io.ReadAll(host); err != nil || !bytes.Equal(received, upload) {
			t.Fatalf("upload: got %d bytes, %v", len(received), err)
		}
		if err := <-sent; err != nil {
			t.Fatal(err)
		}
		if err := client.CloseWrite(); err != nil {
			t.Fatalf("repeat CloseWrite: %v", err)
		}
		if n, err := client.Write([]byte("trailing data")); n != 0 || !errors.Is(err, io.ErrClosedPipe) {
			t.Fatalf("write after FIN: %d, %v", n, err)
		}
		if _, err := host.Write([]byte("published")); err != nil {
			t.Fatal(err)
		}
		if err := host.CloseWrite(); err != nil {
			t.Fatal(err)
		}
		if err := host.Close(); err != nil {
			t.Fatal(err)
		}
		if received, err := io.ReadAll(client); err != nil || string(received) != "published" {
			t.Fatalf("response after peer close: %q, %v", received, err)
		}
	})
}

func TestLibp2pStreamStopsQuietObserverOnlyOnClose(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		client, host := libp2pTestPair(t)
		scoped, release := observeStream(t.Context(), host)
		defer release()
		synctest.Wait()
		if err := client.CloseWrite(); err != nil {
			t.Fatal(err)
		}
		if _, err := host.Read(make([]byte, 1)); err != io.EOF {
			t.Fatalf("logical EOF: %v", err)
		}
		synctest.Wait()
		if scoped.Err() != nil {
			t.Fatal("peer FIN cancelled a quiet call")
		}
		if err := client.Close(); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()
		if scoped.Err() != context.Canceled {
			t.Fatal("peer close did not cancel a quiet call")
		}
	})
}

func TestLibp2pStreamPairingFinish(t *testing.T) {
	t.Run("both_wait_after_finishing", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			client, host := libp2pTestPair(t)
			if err := host.CloseWrite(); err != nil {
				t.Fatal(err)
			}
			if err := client.CloseWrite(); err != nil {
				t.Fatal(err)
			}
			clientStopped := make(chan error, 1)
			go func() {
				_, err := client.WaitStopped(t.Context())
				clientStopped <- err
			}()
			synctest.Wait()
			select {
			case err := <-clientStopped:
				t.Fatalf("response FIN acknowledged the claim by itself: %v", err)
			default:
			}
			if _, err := host.Read(make([]byte, 1)); err != io.EOF {
				t.Fatalf("saved claim acknowledgment: %v", err)
			}
			if code, err := host.WaitStopped(t.Context()); code != nil || err != nil {
				t.Fatalf("host finish: %v, %v", code, err)
			}
			if err := <-clientStopped; err != nil {
				t.Fatal(err)
			}
		})
	})
	t.Run("close_without_fin_is_not_an_acknowledgment", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			client, host := libp2pTestPair(t)
			if _, err := client.Write([]byte("claim")); err != nil {
				t.Fatal(err)
			}
			if err := client.Close(); err != nil {
				t.Fatal(err)
			}
			if claim, err := io.ReadAll(host); string(claim) != "claim" || !errors.Is(err, io.ErrUnexpectedEOF) {
				t.Fatalf("abandoned claim: %q, %v", claim, err)
			}
		})
	})
}

func TestLibp2pStreamCancelledOperationsLeaveStreamUsable(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		left, right := libp2pPipePair(t)
		client := newLibp2pStream(left, nil, nil)
		t.Cleanup(func() { _ = client.Close() })
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		read, written := make(chan error, 1), make(chan error, 1)
		go func() {
			_, err := client.ReadContext(ctx, make([]byte, 1))
			read <- err
		}()
		go func() {
			_, err := client.WriteContext(ctx, []byte("not sent"))
			written <- err
		}()
		synctest.Wait()
		cancel()
		if err := <-read; err != context.Canceled {
			t.Fatalf("cancelled read: %v", err)
		}
		if err := <-written; err != context.Canceled {
			t.Fatalf("cancelled write: %v", err)
		}
		host := newLibp2pStream(right, nil, nil)
		t.Cleanup(func() { _ = host.Close() })
		for _, side := range [][2]*libp2pStream{{client, host}, {host, client}} {
			if _, err := side[0].Write([]byte("retry")); err != nil {
				t.Fatal(err)
			}
			var received [5]byte
			if _, err := io.ReadFull(side[1], received[:]); err != nil || string(received[:]) != "retry" {
				t.Fatalf("read after cancellation: %q, %v", received, err)
			}
		}
	})
}

func TestLibp2pStreamCancelledPartialRecordCannotResume(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		left, right := libp2pPipePair(t)
		stream := newLibp2pStream(left, nil, nil)
		t.Cleanup(func() { _ = stream.Close() })
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		type writeResult struct {
			n   int
			err error
		}
		result := make(chan writeResult, 1)
		go func() {
			n, err := stream.WriteContext(ctx, []byte("payload"))
			result <- writeResult{n, err}
		}()
		var prefixAndPayload [6]byte
		if _, err := io.ReadFull(right, prefixAndPayload[:]); err != nil {
			t.Fatal(err)
		}
		cancel()
		if got := <-result; got.n != 2 || got.err != context.Canceled {
			t.Fatalf("partial write: %d, %v", got.n, got.err)
		}
		if n, err := stream.Write([]byte("new record")); n != 0 || err == nil {
			t.Fatalf("write after truncated record: %d, %v", n, err)
		}
	})
}

func TestLibp2pStreamPropagatesReset(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		left, _ := libp2pPipePair(t)
		left.readErr = &network.StreamError{ErrorCode: 42, Remote: true}
		stream := newLibp2pStream(left, nil, nil)
		t.Cleanup(func() { _ = stream.Close() })
		if _, err := stream.Read(make([]byte, 1)); !errors.Is(err, network.ErrReset) {
			t.Fatalf("read reset: %v", err)
		}
		if code, err := stream.WaitStopped(t.Context()); err != nil || code == nil || *code != 42 {
			t.Fatalf("wait reset: %v, %v", code, err)
		}
		if _, err := stream.Write([]byte("reply")); !errors.Is(err, network.ErrReset) {
			t.Fatalf("write after reset: %v", err)
		}
	})
}

func TestLibp2pStreamDoneReleasesOnce(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		left, _ := libp2pPipePair(t)
		owner, cancel := context.WithCancel(t.Context())
		defer cancel()
		releases := 0
		stream := newLibp2pStream(left, owner, func() { releases++ })
		t.Cleanup(func() { _ = stream.Close() })
		written := make(chan error, 1)
		go func() {
			_, err := stream.Write([]byte("blocked"))
			written <- err
		}()
		synctest.Wait()
		cancel()
		synctest.Wait()
		if err := <-written; !errors.Is(err, io.ErrClosedPipe) {
			t.Fatalf("write after shutdown: %v", err)
		}
		if releases != 1 {
			t.Fatalf("shutdown released stream %d times", releases)
		}
		if err := stream.Close(); err != nil || releases != 1 {
			t.Fatalf("close after shutdown: released %d times, %v", releases, err)
		}
	})
}
