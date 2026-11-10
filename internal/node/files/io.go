package files

import (
	"context"
	"io"
)

func readBounded(ctx context.Context, reader io.Reader, limit int) ([]byte, error) {
	var data []byte
	buffer := make([]byte, 64*1024)
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		size := min(len(buffer), limit+1-len(data))
		if size == 0 {
			return nil, invalid("file exceeds the inspection limit")
		}
		n, err := reader.Read(buffer[:size])
		data = append(data, buffer[:n]...)
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
