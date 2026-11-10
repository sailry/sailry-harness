package hostctl

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func readLine(reader *bufio.Reader, maximum int) ([]byte, error) {
	var line []byte
	for {
		part, err := reader.ReadSlice('\n')
		if len(line)+len(part) > maximum {
			return nil, errors.New("Host control frame exceeds its limit")
		}
		line = append(line, part...)
		if err == nil {
			if !utf8Line(line) {
				return nil, errors.New("invalid Host control frame")
			}
			return line, nil
		}
		if !errors.Is(err, bufio.ErrBufferFull) {
			return nil, err
		}
	}
}

func utf8Line(line []byte) bool {
	return len(line) > 0 && line[len(line)-1] == '\n' && json.Valid(bytes.TrimSpace(line))
}

func writeLine(writer io.Writer, value any, maximum int) error {
	line, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if len(line)+1 > maximum {
		return errors.New("Host control response exceeds its limit")
	}
	line = append(line, '\n')
	for len(line) > 0 {
		count, err := writer.Write(line)
		line = line[count:]
		if err != nil {
			return err
		}
		if count == 0 {
			return io.ErrShortWrite
		}
	}
	return nil
}

func asFault(err error) *protocol.Fault {
	var pointer *protocol.Fault
	if errors.As(err, &pointer) {
		return pointer
	}
	var wrapped *link.FaultError
	if errors.As(err, &wrapped) {
		return &wrapped.Fault
	}
	var value protocol.Fault
	if errors.As(err, &value) {
		return &value
	}
	if errors.Is(err, context.Canceled) {
		return &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "operation cancelled"}
	}
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Host control is unavailable"}
}

func payload[T any](value any) (T, error) {
	if typed, ok := value.(T); ok {
		return typed, nil
	}
	if typed, ok := value.(*T); ok && typed != nil {
		return *typed, nil
	}
	var result T
	return result, fmt.Errorf("unexpected Host response payload")
}
