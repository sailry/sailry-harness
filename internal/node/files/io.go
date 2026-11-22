package files

import (
	"context"
	"io"
)

const boundedReadChunkSize = 64 << 10

func readBounded(ctx context.Context, reader io.Reader, limit int) ([]byte, error) {
	return readBoundedWithScratch(ctx, reader, limit, make([]byte, boundedReadChunkSize))
}

func readBoundedWithScratch(ctx context.Context, reader io.Reader, limit int, scratch []byte) ([]byte, error) {
	var data []byte
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		size := min(len(scratch), limit+1-len(data))
		if size == 0 {
			return nil, invalid("file exceeds the inspection limit")
		}
		n, err := reader.Read(scratch[:size])
		data = append(data, scratch[:n]...)
		if len(data) > limit {
			return nil, invalid("file exceeds the inspection limit")
		}
		if err == io.EOF {
			return data, nil
		}
		if err != nil {
			return nil, err
		}
	}
}
