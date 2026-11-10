package iroh

import (
	"context"
	"io"
	"runtime"
	"sync"
	"sync/atomic"
)

type streamHandles struct {
	Send    uint64 `json:"send"`
	Receive uint64 `json:"recv"`
}

type BiStream struct {
	handles   streamHandles
	closed    atomic.Bool
	closeOnce sync.Once
	closeErr  error
	finish    sync.Once
	finishErr error
}

func (stream *BiStream) Read(data []byte) (int, error) {
	return stream.ReadContext(context.Background(), data)
}

func (stream *BiStream) Write(data []byte) (int, error) {
	return stream.WriteContext(context.Background(), data)
}

func (stream *BiStream) ReadContext(ctx context.Context, data []byte) (int, error) {
	if len(data) == 0 {
		return 0, nil
	}
	if stream.closed.Load() {
		return 0, ErrClosed
	}
	result, err := invoke(ctx, func(functions *abi) uintptr {
		return functions.receiveRead(stream.handles.Receive, uintptr(len(data)))
	})
	if err != nil {
		return 0, err
	}
	if result.status == 3 {
		return 0, io.EOF
	}
	return copy(data, result.data), nil
}

func (stream *BiStream) WriteContext(ctx context.Context, data []byte) (int, error) {
	if len(data) == 0 {
		return 0, nil
	}
	if stream.closed.Load() {
		return 0, ErrClosed
	}
	written := 0
	for written < len(data) {
		var result struct {
			Written int `json:"written"`
		}
		err := invokeJSON(ctx, func(functions *abi) uintptr {
			remaining := data[written:]
			operation := functions.sendWrite(stream.handles.Send, bytesPointer(remaining), uintptr(len(remaining)))
			runtime.KeepAlive(remaining)
			return operation
		}, &result)
		written += result.Written
		if err != nil {
			return written, err
		}
	}
	return written, nil
}

func (stream *BiStream) CloseWrite() error {
	if stream.closed.Load() {
		return ErrClosed
	}
	stream.finish.Do(func() {
		stream.finishErr = invokeJSON(context.Background(), func(functions *abi) uintptr {
			return functions.sendFinish(stream.handles.Send)
		}, nil)
	})
	return stream.finishErr
}

func (stream *BiStream) WaitStopped(ctx context.Context) (*uint64, error) {
	if stream.closed.Load() {
		return nil, ErrClosed
	}
	var result struct {
		Code *uint64 `json:"code"`
	}
	err := invokeJSON(ctx, func(functions *abi) uintptr {
		return functions.sendStopped(stream.handles.Send)
	}, &result)
	return result.Code, err
}

func (stream *BiStream) Reset(code uint64) error {
	if stream.closed.Load() {
		return ErrClosed
	}
	return invokeJSON(context.Background(), func(functions *abi) uintptr {
		return functions.sendReset(stream.handles.Send, code)
	}, nil)
}

func (stream *BiStream) Stop(code uint64) error {
	if stream.closed.Load() {
		return ErrClosed
	}
	return invokeJSON(context.Background(), func(functions *abi) uintptr {
		return functions.receiveStop(stream.handles.Receive, code)
	}, nil)
}

func (stream *BiStream) Close() error {
	stream.closeOnce.Do(func() {
		stream.closed.Store(true)
		stream.closeErr = invokeJSON(context.Background(), func(functions *abi) uintptr {
			return functions.streamDrop(stream.handles.Send, stream.handles.Receive)
		}, nil)
	})
	return stream.closeErr
}
