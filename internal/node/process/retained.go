package process

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type outputBudget struct {
	used  atomic.Int64
	limit int64
}

func (budget *outputBudget) available() int64 { return max(budget.limit-budget.used.Load(), 0) }

func (budget *outputBudget) take(requested int) int {
	for {
		previous := budget.used.Load()
		count := min(int64(requested), max(budget.limit-previous, 0))
		if budget.used.CompareAndSwap(previous, previous+count) {
			return int(count)
		}
	}
}

type retainedStream struct {
	mu       sync.Mutex
	file     *os.File
	path     string
	error    error
	limit    int
	reserved int
	retained int
	total    uint64
	written  uint64
	tail     []byte
	finished bool
	budget   *outputBudget
	capture  outputBuffer
	decoder  outputDecoder
	scanner  serviceScanner
	toScan   []byte
	scanned  uint64
}

func newRetained(profile string, budget *outputBudget, tail, command bool) *retainedStream {
	prefix, suffix := captureLimit, 0
	if tail {
		prefix, suffix = 0, captureLimit
	}
	stream := &retainedStream{budget: budget, capture: outputBuffer{tail: tail, decoder: newOutputDecoder(prefix, suffix, command)}}
	directory := filepath.Join(profile, "tmp")
	if err := os.MkdirAll(directory, 0700); err != nil {
		stream.error = err
		return stream
	}
	stream.file, stream.error = os.CreateTemp(directory, "command-output-")
	if stream.error == nil {
		stream.path = stream.file.Name()
		stream.limit = budget.take(streamLimit)
		stream.reserved = stream.limit
	}
	stream.decoder = newOutputDecoder(min(stream.limit, prefixLimit), max(stream.limit-prefixLimit, recentLimit), command)
	return stream
}

func clearOutput(profile string) error {
	directory := filepath.Join(profile, "tmp")
	entries, err := os.ReadDir(directory)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), "command-output-") && (entry.Type().IsRegular() || entry.Type()&os.ModeSymlink != 0) {
			if err := os.Remove(filepath.Join(directory, entry.Name())); err != nil {
				return err
			}
		}
	}
	return nil
}

func (stream *retainedStream) append(value []byte) {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	stream.capture.append(value)
	stream.decoder.append(value)
	stream.toScan = append(stream.toScan, value...)
	stream.scan(false)
	start := stream.total
	stream.total += uint64(len(value))
	if len(value) >= recentLimit {
		stream.tail = append(stream.tail[:0], value[len(value)-recentLimit:]...)
	} else {
		discard := max(len(stream.tail)+len(value)-recentLimit, 0)
		stream.tail = append(stream.tail[discard:], value...)
	}
	if len(value) == 0 || stream.error != nil || stream.file == nil {
		return
	}
	head := min(stream.limit, prefixLimit)
	capacity := stream.limit - head
	cursor := start
	pending := value
	write := func(value []byte, offset uint64) bool {
		if _, err := stream.file.WriteAt(value, int64(offset)); err != nil {
			stream.fail(err)
			return false
		}
		return true
	}
	if cursor < uint64(head) {
		count := min(len(pending), head-int(cursor))
		if !write(pending[:count], cursor) {
			return
		}
		cursor += uint64(count)
		pending = pending[count:]
	}
	if capacity > 0 && len(pending) > 0 {
		skip := max(len(pending)-capacity, 0)
		pending = pending[skip:]
		cursor += uint64(skip)
		position := int((cursor - uint64(head)) % uint64(capacity))
		count := min(len(pending), capacity-position)
		if !write(pending[:count], uint64(head+position)) {
			return
		}
		if count < len(pending) && !write(pending[count:], uint64(head)) {
			return
		}
	}
	stream.retained = int(min(uint64(stream.limit), stream.total))
	stream.written = stream.total
	if capacity == 0 {
		stream.written = min(stream.total, uint64(head))
	}
}

func (stream *retainedStream) fail(err error) {
	stream.error = err
	stream.budget.used.Add(-int64(stream.reserved))
	stream.reserved, stream.retained = 0, 0
	if stream.file != nil {
		stream.file.Close()
		stream.file = nil
	}
	if stream.path != "" {
		os.Remove(stream.path)
		stream.path = ""
	}
}

func (stream *retainedStream) finish() {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	if stream.finished {
		return
	}
	stream.finished, stream.capture.finished = true, true
	stream.decoder.finish()
	stream.capture.decoder.finish()
	stream.scan(true)
	stream.budget.used.Add(-int64(stream.reserved - stream.retained))
	stream.reserved = stream.retained
	if stream.file != nil {
		stream.file.Close()
		stream.file = nil
	}
}

func (stream *retainedStream) evict() {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	stream.fail(errors.New("command output was evicted"))
	if stream.path != "" {
		os.Remove(stream.path)
		stream.path = ""
	}
}

func (stream *retainedStream) bytes() int {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	return stream.retained
}

func (stream *retainedStream) snapshot(recent bool) protocol.ProcessCapture {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	if !recent {
		return stream.capture.capture()
	}
	value := stream.tail
	offset := stream.total - uint64(len(value))
	start, end := stream.decoder.bounds(value, offset, stream.finished)
	text, invalid := stream.decoder.decode(value[start:end], offset+uint64(start))
	return protocol.ProcessCapture{Text: text, Truncated: stream.total > uint64(len(value)), InvalidUtf8: invalid}
}

func (stream *retainedStream) read(offset uint64, count int) ([]byte, error) {
	if count == 0 {
		return []byte{}, nil
	}
	file := stream.file
	if file == nil {
		var err error
		file, err = os.Open(stream.path)
		if err != nil {
			return nil, err
		}
		defer file.Close()
	}
	head := min(stream.limit, prefixLimit)
	capacity := stream.limit - head
	value := make([]byte, count)
	for cursor := 0; cursor < count; {
		logical := offset + uint64(cursor)
		physical := int(logical)
		if logical >= uint64(head) {
			physical = head + int((logical-uint64(head))%uint64(capacity))
		}
		available := stream.limit - physical
		if physical < head {
			available = head - physical
		}
		size := min(count-cursor, available)
		if _, err := file.ReadAt(value[cursor:cursor+size], int64(physical)); err != nil {
			return nil, err
		}
		cursor += size
	}
	return value, nil
}

// ranges reads character-aligned logical ranges while stream.mu is held.
func (stream *retainedStream) ranges() (head, tail, end uint64, err error) {
	head = uint64(min(stream.retained, min(stream.limit, prefixLimit)))
	tailBytes := uint64(stream.retained) - head
	tail = stream.written - tailBytes
	end = head
	if tailBytes > 0 {
		end = stream.written
	}
	gap := tail > head && end > head
	if gap || tailBytes == 0 && stream.total > head {
		if stream.decoder.native() {
			for head > 0 && stream.decoder.split(head) {
				head--
			}
		} else {
			start := head - min(head, 3)
			suffix, err := stream.read(start, int(head-start))
			if err != nil {
				return 0, 0, 0, err
			}
			head = start + uint64(utf8End(suffix))
		}
	}
	if gap {
		if stream.decoder.native() {
			for tail < stream.written && stream.decoder.split(tail) {
				tail++
			}
		} else {
			beginning, err := stream.read(tail, int(min(tailBytes, 4)))
			if err != nil {
				return 0, 0, 0, err
			}
			tail += uint64(utf8Start(beginning))
		}
	}
	if tailBytes == 0 {
		end = head
	}
	return head, tail, end, nil
}

func (stream *retainedStream) page(info protocol.ProcessInfo, kind protocol.ProcessOutputOutputStream, offset, limit uint64) (protocol.ProcessOutputOutputPage, error) {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	if stream.error != nil {
		return protocol.ProcessOutputOutputPage{}, outputFault(stream.error)
	}
	if limit == 0 {
		return protocol.ProcessOutputOutputPage{}, invalid("command output page is invalid")
	}
	head, tail, end, err := stream.ranges()
	if err != nil {
		return protocol.ProcessOutputOutputPage{}, outputFault(err)
	}
	gap := tail > head && end > head
	if gap && offset >= head && offset < tail {
		offset = tail
	}
	pageEndOffset := end
	if gap && offset < head {
		pageEndOffset = head
	} else if offset > end || gap && offset < tail {
		return protocol.ProcessOutputOutputPage{}, invalid("command output offset is outside retained ranges")
	}
	value, err := stream.read(offset, int(min(pageEndOffset-offset, min(limit, outputPage)+4)))
	if err != nil {
		return protocol.ProcessOutputOutputPage{}, outputFault(err)
	}
	count, err := stream.decoder.page(value, offset, int(min(limit, outputPage)), stream.finished && pageEndOffset == stream.written)
	if err != nil {
		return protocol.ProcessOutputOutputPage{}, err
	}
	text, bad := stream.decoder.decode(value[:count], offset)
	next := offset + uint64(count)
	var nextOffset *uint64
	if next < pageEndOffset {
		nextOffset = &next
	} else if gap && next == head {
		nextOffset = &tail
	}
	return protocol.ProcessOutputOutputPage{Info: info, Stream: kind, Offset: offset, TotalBytes: stream.total, NextOffset: nextOffset, Text: text, Truncated: stream.total > uint64(stream.retained), InvalidUtf8: bad}, nil
}

func outputFault(err error) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: fmt.Sprintf("retained command output is unavailable: %v", err)}
}

func (stream *retainedStream) scan(complete bool) {
	start, end := stream.decoder.bounds(stream.toScan, stream.scanned, complete)
	text, _ := stream.decoder.decode(stream.toScan[start:end], stream.scanned+uint64(start))
	stream.scanner.append([]byte(text))
	stream.toScan = append(stream.toScan[:0], stream.toScan[end:]...)
	stream.scanned += uint64(end)
}

func (stream *retainedStream) services() []protocol.ProcessService {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	return slices.Clone(stream.scanner.found)
}
