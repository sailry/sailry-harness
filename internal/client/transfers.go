package client

import (
	"bytes"
	"context"
	"encoding/hex"
	"errors"
	"io"
	"sync/atomic"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type transferClock interface {
	AfterFunc(time.Duration, func()) func() bool
}
type systemTransferClock struct{}

func (systemTransferClock) AfterFunc(d time.Duration, call func()) func() bool {
	return time.AfterFunc(d, call).Stop
}

// Upload stages source bytes. The caller retains the durable finish request to
// publish them. Cancellation or stalled IO closes the borrowed source to stop IO;
// successful transfers leave its ownership with the caller.
func (client *Client) Upload(ctx context.Context, id protocol.StreamID, size uint64, revision string, source io.ReadCloser, progress func(uint64)) (err error) {
	defer func() {
		if err != nil {
			client.cancelTransfer(id)
		}
	}()
	expected, err := transferDigest(revision)
	if err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return transferCancelled()
	}
	stream, err := client.transport.Open(ctx, id)
	if err != nil {
		return err
	}
	defer stream.Close()
	closeIO := func() { _ = stream.Close(); _ = source.Close() }
	stopCancellation := transferCancellation(ctx, closeIO)
	defer stopCancellation()
	digest := blake3.New(32, nil)
	data := make([]byte, protocol.FILE_TRANSFER_CHUNK_BYTES)
	var total uint64
	if progress != nil {
		progress(0)
	}
	for {
		var count int
		err = client.transferStep(ctx, closeIO, func() error { var e error; count, e = source.Read(data); return e })
		if err != nil && !errors.Is(err, io.EOF) {
			return err
		}
		if count > 0 {
			if uint64(count) > size-total {
				return transferChanged()
			}
			total += uint64(count)
			if writeErr := client.transferStep(ctx, closeIO, func() error { return writeTransferBytes(stream, data[:count]) }); writeErr != nil {
				return writeErr
			}
			_, _ = digest.Write(data[:count])
			if progress != nil {
				progress(total)
			}
		}
		if errors.Is(err, io.EOF) {
			break
		}
	}
	if total != size || !bytes.Equal(digest.Sum(nil), expected) {
		return transferChanged()
	}
	return client.transferStep(ctx, closeIO, func() error {
		if err := stream.CloseWrite(); err != nil {
			return err
		}
		var ready [1]byte
		if _, err := io.ReadFull(stream, ready[:]); err != nil {
			return err
		}
		if ready[0] != protocol.FILE_UPLOAD_READY {
			return io.ErrUnexpectedEOF
		}
		count, err := stream.Read(ready[:])
		if count != 0 || err != io.EOF {
			return io.ErrUnexpectedEOF
		}
		return nil
	})
}

// Download receives into caller-owned staging. A failed transfer can leave
// partial bytes; the caller publishes the destination only after success.
// Cancellation or stalled IO closes the borrowed destination to stop IO.
func (client *Client) Download(ctx context.Context, id protocol.StreamID, size uint64, revision string, destination io.WriteCloser, progress func(uint64)) (err error) {
	defer func() {
		if err != nil {
			client.cancelTransfer(id)
		}
	}()
	expected, err := transferDigest(revision)
	if err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return transferCancelled()
	}
	stream, err := client.transport.Open(ctx, id)
	if err != nil {
		return err
	}
	defer stream.Close()
	closeIO := func() { _ = stream.Close(); _ = destination.Close() }
	stopCancellation := transferCancellation(ctx, closeIO)
	defer stopCancellation()
	if err = client.transferStep(ctx, closeIO, stream.CloseWrite); err != nil {
		return err
	}
	digest := blake3.New(32, nil)
	data := make([]byte, protocol.FILE_TRANSFER_CHUNK_BYTES)
	var total uint64
	if progress != nil {
		progress(0)
	}
	for {
		var count int
		err = client.transferStep(ctx, closeIO, func() error { var e error; count, e = stream.Read(data); return e })
		if err != nil && !errors.Is(err, io.EOF) {
			return err
		}
		if count > 0 {
			if uint64(count) > size-total {
				return transferChanged()
			}
			total += uint64(count)
			if writeErr := client.transferStep(ctx, closeIO, func() error { return writeTransferBytes(destination, data[:count]) }); writeErr != nil {
				return writeErr
			}
			_, _ = digest.Write(data[:count])
			if progress != nil {
				progress(total)
			}
		}
		if errors.Is(err, io.EOF) {
			break
		}
	}
	if total != size || !bytes.Equal(digest.Sum(nil), expected) {
		return transferChanged()
	}
	// Flush is the writer's publication preparation, not an additional copy.
	if buffered, ok := destination.(interface{ Flush() error }); ok {
		return client.transferStep(ctx, closeIO, buffered.Flush)
	}
	return nil
}

func transferCancellation(ctx context.Context, closeIO func()) func() {
	done := make(chan struct{})
	stop := context.AfterFunc(ctx, func() { defer close(done); closeIO() })
	return func() {
		if !stop() {
			<-done
		}
	}
}

func (client *Client) transferStep(ctx context.Context, closeIO func(), operation func() error) error {
	if ctx.Err() != nil {
		return transferCancelled()
	}
	var expired atomic.Bool
	done := make(chan struct{})
	stop := client.transferClock.AfterFunc(30*time.Second, func() { defer close(done); expired.Store(true); closeIO() })
	err := operation()
	if !stop() {
		<-done
	}
	if ctx.Err() != nil {
		return transferCancelled()
	}
	if expired.Load() {
		return fault(protocol.ErrorCodeUnavailable, "file transfer made no progress")
	}
	if err != nil && !errors.Is(err, io.EOF) {
		return fault(protocol.ErrorCodeUnavailable, "file transfer stream failed")
	}
	return err
}

func (client *Client) cancelTransfer(id protocol.StreamID) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	stop := client.transferClock.AfterFunc(2*time.Second, cancel)
	defer stop()
	_, _ = client.Do(ctx, protocol.Command{Kind: "cancel_file_transfer", Data: &protocol.CommandCancelFileTransferPayload{Stream: id}})
}
func transferDigest(revision string) ([]byte, error) {
	value, err := hex.DecodeString(revision)
	if err != nil || len(value) != 32 {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "invalid transfer revision")
	}
	return value, nil
}
func transferChanged() error {
	return fault(protocol.ErrorCodeRevisionConflict, "file transfer size or content changed; partial data must not be published")
}
func transferCancelled() error { return fault(protocol.ErrorCodeCancelled, "file transfer cancelled") }
func writeTransferBytes(writer io.Writer, data []byte) error {
	for len(data) > 0 {
		n, err := writer.Write(data)
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
