package hostctl

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func RunTask(ctx context.Context, options TaskOptions, output io.Writer) (int, error) {
	encoded, err := json.Marshal(options.Task)
	if err != nil {
		return 1, err
	}
	if len(encoded)+1 > MaxTaskRequest {
		return 1, errors.New("Host task request exceeds its limit")
	}
	known := options.Task.Result()
	print := func(result TaskResult) (int, error) {
		err := writeLine(output, result, MaxTaskResponse)
		return result.ExitCode(), err
	}
	connection, err := connectControl(ctx, options.Profile)
	if err != nil {
		return print(known.failed(&protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Host is not running"}))
	}
	defer connection.Close()
	stop := context.AfterFunc(ctx, func() { _ = connection.Close() })
	defer stop()
	if err := writeLine(connection, options.Task, MaxTaskRequest); err != nil {
		if ctx.Err() != nil {
			known.State = "observation_cancelled"
			return print(known)
		}
		return print(known.uncertain("Host task submission is unconfirmed; retain the same request identifier"))
	}
	reader := bufio.NewReader(connection)
	for {
		line, err := readLine(reader, MaxTaskResponse)
		if ctx.Err() != nil {
			known.State = "observation_cancelled"
			return print(known)
		}
		if err != nil {
			return print(known.uncertain("Host task observation disconnected; retain the same request or turn identifier"))
		}
		var incoming TaskResult
		if json.Unmarshal(line, &incoming) != nil || !matchesResponse(known, incoming) {
			return print(known.uncertain("Host task observation returned an invalid response or mismatched identifiers"))
		}
		if (incoming.State == "admitted" || incoming.State == "observing") && options.Wait {
			known = incoming
			continue
		}
		return print(incoming)
	}
}
