package output

import "github.com/sailry/sailry-harness/internal/protocol"

func processedCommand(fields, data map[string]any, size int) map[string]any {
	if data["wait"] != nil {
		return nil
	}
	outcome := object(data["outcome"])
	var exitCode any
	switch outcome["kind"] {
	case "exited":
		if len(outcome) == 2 {
			exitCode = outcome["data"]
		}
	case nil:
		if len(outcome) == 1 {
			exitCode = outcome["exit_code"]
		}
	}
	if code, ok := commandUint(exitCode); !ok || code != 0 {
		return nil
	}
	retained := object(fields["sailry_command"])
	streamRefs := map[string]any{}
	id := retained["id"]
	if id == nil {
		id = object(data["info"])["id"]
	}
	if id == nil {
		id = object(fields["info"])["id"]
	}
	if id != nil || fields["sailry_command"] != nil {
		id := text(id)
		if _, err := protocol.ParseRequestID(id); err != nil {
			return nil
		}
		streamRefs["id"] = id
	}
	result := map[string]any{"outcome": outcome}
	if elapsed, ok := commandUint(data["elapsed_ms"]); ok {
		result["elapsed_ms"] = elapsed
	}
	captured := 0
	for _, stream := range []string{"stdout", "stderr"} {
		capture := object(data[stream])
		body, ok := capture["text"].(string)
		truncated, hasTruncation := capture["truncated"].(bool)
		if !ok || !hasTruncation || truncated && streamRefs["id"] == nil {
			return nil
		}
		facts := map[string]any{"bytes": len(body), "truncated": truncated}
		if stream == "stderr" && body != "" {
			if len(body) > 256 {
				return nil
			}
			facts["text"] = body
		}
		if invalid, ok := capture["invalid_utf8"].(bool); ok {
			facts["invalid_utf8"] = invalid
		}
		result[stream] = facts
		captured += len(body)
		if preview := object(retained[stream]); preview != nil {
			bounds := map[string]any{}
			for _, key := range []string{"head_bytes", "tail_offset", "total_bytes", "retained_bytes", "retained_head_bytes", "retained_tail_offset", "retained_end"} {
				if n, ok := commandUint(preview[key]); ok {
					bounds[key] = n
				}
			}
			for _, key := range []string{"head_invalid_utf8", "invalid_utf8", "transcoded"} {
				if flag, ok := preview[key].(bool); ok {
					bounds[key] = flag
				}
			}
			streamRefs[stream] = bounds
		}
	}
	if captured == 0 {
		return nil
	}
	summary := map[string]any{"processed": "command_result", "bytes": size, "data": result}
	if len(streamRefs) != 0 {
		summary["sailry_command"] = streamRefs
	}
	return summary
}
