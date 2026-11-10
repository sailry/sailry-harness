package process

import "github.com/sailry/sailry-harness/internal/protocol"

// Preview freezes bounded model metadata without changing the command capture.
// Head is requested only for background snapshots, whose captures own the tail.
func (s *Service) Preview(session protocol.SessionID, id protocol.RequestID, head bool) (map[string]any, error) {
	s.mu.Lock()
	entry, err := s.entryLocked(session, id)
	if err != nil {
		s.mu.Unlock()
		return nil, err
	}
	info, stdout, stderr := entry.info, entry.stdout, entry.stderr
	s.mu.Unlock()
	value := map[string]any{"id": id}
	for _, item := range []struct {
		name   string
		kind   protocol.ProcessOutputOutputStream
		stream *retainedStream
	}{{"stdout", protocol.ProcessOutputOutputStreamStdout, stdout}, {"stderr", protocol.ProcessOutputOutputStreamStderr, stderr}} {
		preview := item.stream.preview()
		if preview == nil {
			continue
		}
		if head {
			if page, err := item.stream.page(info, item.kind, 0, recentLimit); err == nil {
				if !page.InvalidUtf8 && preview["transcoded"] != true {
					preview["head_bytes"] = len(page.Text)
				}
				preview["head_invalid_utf8"], preview["head"] = page.InvalidUtf8, page.Text
			}
		}
		value[item.name] = preview
	}
	if len(value) > 1 {
		for _, name := range []string{"stdout", "stderr"} {
			if _, present := value[name]; !present {
				value[name] = nil
			}
		}
	}
	return value, nil
}

func (stream *retainedStream) preview() map[string]any {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	transcoded := stream.decoder.transcoded()
	if stream.total <= captureLimit {
		if transcoded {
			return map[string]any{"transcoded": true, "total_bytes": stream.total}
		}
		return nil
	}
	offset := stream.total - uint64(len(stream.tail))
	start, end := stream.decoder.bounds(stream.tail, offset, stream.finished)
	offset += uint64(start)
	text, invalid := stream.decoder.decode(stream.tail[start:end], offset)
	head := uint64(min(stream.retained, min(stream.limit, prefixLimit)))
	tail, written := stream.written-(uint64(stream.retained)-head), stream.written
	if first, last, end, err := stream.ranges(); err == nil {
		head, tail, written = first, last, end
	}
	return map[string]any{
		"tail": text, "tail_offset": offset, "transcoded": transcoded,
		"total_bytes": stream.total, "retained_bytes": stream.retained,
		"retained_head_bytes": head, "retained_tail_offset": tail,
		"retained_end": written, "invalid_utf8": invalid,
	}
}
