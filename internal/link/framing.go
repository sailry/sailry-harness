package link

import (
	"context"
	"io"

	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

type contextStream interface {
	Stream
	ReadContext(context.Context, []byte) (int, error)
	WriteContext(context.Context, []byte) (int, error)
	WaitStopped(context.Context) (*uint64, error)
}

type scopedReader struct {
	ctx    context.Context
	stream contextStream
}

func (reader scopedReader) Read(bytes []byte) (int, error) {
	return reader.stream.ReadContext(reader.ctx, bytes)
}

type scopedWriter struct {
	ctx    context.Context
	stream contextStream
}

func (writer scopedWriter) Write(bytes []byte) (int, error) {
	return writer.stream.WriteContext(writer.ctx, bytes)
}

func (link *Link) writeCall(ctx context.Context, stream contextStream, call wire.Call, compressed bool) error {
	scoped, cancel := link.operation(ctx, true)
	defer cancel()
	return wire.Write(scopedWriter{scoped, stream}, link.envelope(call), compressed)
}

func (link *Link) readFrame(ctx context.Context, stream contextStream, value any, compressed bool) error {
	scoped, cancel := link.operation(ctx, true)
	defer cancel()
	return wire.Read(scopedReader{scoped, stream}, value, compressed)
}

func (link *Link) writeFrame(ctx context.Context, stream contextStream, value any) error {
	scoped, cancel := link.operation(ctx, true)
	defer cancel()
	return wire.Write(scopedWriter{scoped, stream}, value, false)
}

func (link *Link) writeReply(ctx context.Context, stream contextStream, reply wire.Reply, compressed bool) error {
	scoped, cancel := link.operation(ctx, true)
	defer cancel()
	return wire.WriteReply(scopedWriter{scoped, stream}, reply, compressed)
}

func copyDuplex(ctx context.Context, first, second Stream) error {
	results := make(chan error, 2)
	copyDirection := func(destination, source Stream) {
		_, err := io.CopyBuffer(destination, source, make([]byte, 8192))
		if err == nil {
			err = destination.CloseWrite()
		}
		results <- err
	}
	go copyDirection(first, second)
	go copyDirection(second, first)
	var firstError error
	for count := 0; count < 2; count++ {
		select {
		case <-ctx.Done():
			_ = first.Close()
			_ = second.Close()
			for ; count < 2; count++ {
				<-results
			}
			return linkNetwork(ctx.Err())
		case err := <-results:
			if err != nil && firstError == nil {
				firstError = err
				_ = first.Close()
				_ = second.Close()
			}
		}
	}
	return linkNetwork(firstError)
}
