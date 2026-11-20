package link

import (
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"sync"
	"sync/atomic"
	"time"

	"github.com/libp2p/go-libp2p/core/network"
)

const (
	libp2pRecordLimit        = 32 << 10
	libp2pFinishAck   uint32 = 0xffffffff
	libp2pRevokedCode        = network.StreamErrorCode(0x53414901)
)

var (
	errLibp2pAccessRevoked = errors.New("peer access revoked")
	libp2pRevokedByPeer    = &network.StreamError{Remote: true, ErrorCode: libp2pRevokedCode}
)

// Records carry a big-endian u32 payload size; zero finishes the application
// write half without closing the transport. Only that record yields io.EOF.
// The portable libp2p stream has no way to separately observe a peer stop.
type libp2pStream struct {
	raw     network.Stream
	owner   context.Context
	release func()

	reading chan struct{}
	data    chan []byte
	pending []byte
	readErr error
	peerFIN chan struct{}
	peerAck chan struct{}
	stopped chan struct{}
	stopErr error

	writing  chan struct{}
	finished atomic.Bool
	ackSent  bool
	writeErr error

	closed    chan struct{}
	closeOnce sync.Once
	closeErr  error
}

var _ contextStream = (*libp2pStream)(nil)

func newLibp2pStream(raw network.Stream, owner context.Context, release func()) *libp2pStream {
	stream := &libp2pStream{
		raw: raw, owner: owner, release: release,
		reading: make(chan struct{}, 1), data: make(chan []byte, 1),
		peerFIN: make(chan struct{}), peerAck: make(chan struct{}), stopped: make(chan struct{}),
		writing: make(chan struct{}, 1), closed: make(chan struct{}),
	}
	go stream.pump()
	if owner != nil {
		go func() {
			select {
			case <-owner.Done():
				_ = stream.Close()
			case <-stream.closed:
			}
		}()
	}
	return stream
}

func (stream *libp2pStream) Read(data []byte) (int, error) {
	return stream.ReadContext(context.Background(), data)
}

func (stream *libp2pStream) ReadContext(ctx context.Context, data []byte) (int, error) {
	if len(data) == 0 {
		return 0, nil
	}
	if err := stream.acquire(ctx, stream.reading); err != nil {
		return 0, err
	}
	defer func() { <-stream.reading }()
	if len(stream.pending) == 0 {
		select {
		case <-ctx.Done():
			return 0, ctx.Err()
		case <-stream.closed:
			return 0, io.ErrClosedPipe
		case chunk, open := <-stream.data:
			if !open {
				return 0, stream.readErr
			}
			stream.pending = chunk
		}
	}
	n := copy(data, stream.pending)
	if n == len(stream.pending) {
		stream.pending = nil
	} else {
		stream.pending = stream.pending[n:]
	}
	return n, nil
}

func (stream *libp2pStream) pump() {
	err := stream.pumpData()
	stream.readErr = err
	close(stream.data)
	if err == io.EOF {
		close(stream.peerFIN)
		err = stream.pumpStop()
		if errors.Is(err, io.EOF) {
			err = io.ErrClosedPipe
		}
	}
	stream.stopErr = err
	close(stream.stopped)
	// Some muxers do not signal that the peer closed its read half.
	_ = stream.raw.SetWriteDeadline(time.Now())
}

// The pump is the only raw reader. A bounded queue leaves application bytes
// with ReadContext while letting a quiet request reach its FIN and peer stop.
func (stream *libp2pStream) pumpData() error {
	var header [4]byte
	for {
		if _, err := io.ReadFull(stream.raw, header[:]); err != nil {
			return libp2pUnexpectedEOF(err)
		}
		size := binary.BigEndian.Uint32(header[:])
		if size == 0 {
			return io.EOF
		}
		if size > libp2pRecordLimit {
			return fmt.Errorf("invalid libp2p stream record size %d", size)
		}
		chunk := make([]byte, size)
		n, err := io.ReadFull(stream.raw, chunk)
		if n != 0 {
			select {
			case stream.data <- chunk[:n]:
			case <-stream.closed:
				return io.ErrClosedPipe
			}
		}
		if err != nil {
			return libp2pUnexpectedEOF(err)
		}
	}
}

func libp2pUnexpectedEOF(err error) error {
	if errors.Is(err, io.EOF) {
		return io.ErrUnexpectedEOF
	}
	return err
}

func (stream *libp2pStream) pumpStop() error {
	var header [4]byte
	if _, err := io.ReadFull(stream.raw, header[:]); err != nil {
		return err
	}
	if binary.BigEndian.Uint32(header[:]) == libp2pFinishAck {
		close(stream.peerAck)
		if _, err := io.ReadFull(stream.raw, header[:1]); err != nil {
			return err
		}
	}
	return errors.New("unexpected libp2p stream data after FIN")
}

func (stream *libp2pStream) Write(data []byte) (int, error) {
	return stream.WriteContext(context.Background(), data)
}

func (stream *libp2pStream) WriteContext(ctx context.Context, data []byte) (int, error) {
	if len(data) == 0 {
		return 0, nil
	}
	return stream.withWriter(ctx, func() (int, error) {
		if stream.finished.Load() {
			return 0, io.ErrClosedPipe
		}
		written := 0
		for written < len(data) {
			if err := ctx.Err(); err != nil {
				return written, err
			}
			remaining := data[written:]
			chunk := remaining[:min(len(remaining), libp2pRecordLimit)]
			n, err := stream.writeRecord(uint32(len(chunk)), chunk)
			written += n
			if err != nil {
				return written, err
			}
		}
		return written, nil
	})
}

func (stream *libp2pStream) CloseWrite() error {
	if channelClosed(stream.closed) {
		return io.ErrClosedPipe
	}
	if stream.finished.Load() {
		return nil
	}
	_, err := stream.withWriter(context.Background(), func() (int, error) {
		if stream.finished.Load() {
			return 0, nil
		}
		n, err := stream.writeRecord(0, nil)
		if err == nil {
			stream.finished.Store(true)
		}
		return n, err
	})
	return err
}

// A passive wait observes abandonment, even after the peer sends its logical
// FIN. Pairing explicitly waits after finishing on both sides: each then sends
// an acknowledgment of the peer's FIN, without either side closing first.
func (stream *libp2pStream) WaitStopped(ctx context.Context) (*uint64, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if channelClosed(stream.closed) {
		return nil, io.ErrClosedPipe
	}
	var acknowledged <-chan struct{}
	if stream.finished.Load() {
		acknowledged = stream.peerAck
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-stream.closed:
			return nil, io.ErrClosedPipe
		case <-stream.stopped:
		case <-stream.peerFIN:
			_, err := stream.withWriter(ctx, func() (int, error) {
				if stream.ackSent {
					return 0, nil
				}
				n, err := stream.writeRecord(libp2pFinishAck, nil)
				if err == nil {
					stream.ackSent = true
				}
				return n, err
			})
			if err != nil && !channelClosed(stream.stopped) && !channelClosed(acknowledged) {
				return nil, err
			}
		}
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-stream.closed:
		return nil, io.ErrClosedPipe
	case <-acknowledged:
		return nil, nil
	case <-stream.stopped:
		if channelClosed(acknowledged) {
			return nil, nil
		}
		var reset *network.StreamError
		if errors.As(stream.stopErr, &reset) && reset.Remote {
			code := uint64(reset.ErrorCode)
			return &code, nil
		}
		return nil, stream.stopErr
	}
}

func (stream *libp2pStream) acquire(ctx context.Context, gate chan struct{}) error {
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-stream.closed:
		return io.ErrClosedPipe
	case gate <- struct{}{}:
	}
	if err := ctx.Err(); err != nil {
		<-gate
		return err
	}
	if channelClosed(stream.closed) {
		<-gate
		return io.ErrClosedPipe
	}
	return nil
}

func (stream *libp2pStream) withWriter(ctx context.Context, write func() (int, error)) (int, error) {
	if err := stream.acquire(ctx, stream.writing); err != nil {
		return 0, err
	}
	defer func() { <-stream.writing }()
	if stream.writeErr != nil {
		return 0, stream.writeErr
	}
	if channelClosed(stream.stopped) {
		return 0, stream.stopErr
	}
	if ctx.Done() != nil {
		interrupted := make(chan struct{})
		stop := context.AfterFunc(ctx, func() {
			_ = stream.raw.SetWriteDeadline(time.Now())
			close(interrupted)
		})
		defer func() {
			if !stop() {
				<-interrupted
				_ = stream.raw.SetWriteDeadline(time.Time{})
			}
		}()
	}
	n, err := write()
	if err != nil {
		if canceled := ctx.Err(); canceled != nil {
			return n, canceled
		}
		if channelClosed(stream.closed) {
			return n, io.ErrClosedPipe
		}
	}
	return n, err
}

func (stream *libp2pStream) writeRecord(size uint32, data []byte) (int, error) {
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], size)
	n, err := stream.raw.Write(header[:])
	if err == nil && n != len(header) {
		err = io.ErrShortWrite
	}
	if err != nil {
		if n != 0 {
			stream.writeErr = err
		}
		return 0, err
	}
	if len(data) == 0 {
		return 0, nil
	}
	n, err = stream.raw.Write(data)
	if n != len(data) {
		if err == nil {
			err = io.ErrShortWrite
		}
		stream.writeErr = err
	}
	return n, err
}

func (stream *libp2pStream) Close() error {
	stream.closeOnce.Do(func() {
		close(stream.closed)
		_ = stream.raw.SetWriteDeadline(time.Now())
		stream.writing <- struct{}{}
		if stream.owner != nil && context.Cause(stream.owner) == errLibp2pAccessRevoked {
			// A path can also carry pairing and the other Link direction.
			// Report revocation on this stream without closing the shared path.
			stream.closeErr = stream.raw.ResetWithError(libp2pRevokedCode)
		} else if stream.writeErr != nil {
			// A partial record cannot be resumed as a new record.
			stream.closeErr = stream.raw.Reset()
		} else {
			// Flush complete records even if the application never sent FIN.
			stream.closeErr = stream.raw.Close()
		}
		<-stream.writing
		<-stream.stopped
		if stream.release != nil {
			stream.release()
		}
	})
	return stream.closeErr
}
