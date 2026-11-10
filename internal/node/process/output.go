package process

import (
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	captureLimit = 64 * 1024
	streamLimit  = 4 * 1024 * 1024
	prefixLimit  = 1024 * 1024
	commandLimit = 2 * streamLimit
	totalLimit   = 256 * 1024 * 1024
	outputPage   = 32 * 1024
	recentLimit  = 8 * 1024
)

type outputBuffer struct {
	bytes     []byte
	tail      bool
	truncated bool
	finished  bool
	total     uint64
	decoder   outputDecoder
}

func (buffer *outputBuffer) append(value []byte) {
	buffer.total += uint64(len(value))
	buffer.decoder.append(value)
	if buffer.tail {
		buffer.bytes = append(buffer.bytes, value...)
		if len(buffer.bytes) > captureLimit {
			buffer.bytes = append(buffer.bytes[:0], buffer.bytes[len(buffer.bytes)-captureLimit:]...)
			buffer.truncated = true
		}
		return
	}
	count := min(len(value), captureLimit-len(buffer.bytes))
	buffer.bytes = append(buffer.bytes, value[:count]...)
	buffer.truncated = buffer.truncated || count < len(value)
}

func (buffer *outputBuffer) capture() protocol.ProcessCapture {
	value := buffer.bytes
	offset := uint64(0)
	if buffer.tail {
		offset = buffer.total - uint64(len(value))
	}
	start, end := buffer.decoder.bounds(value, offset, buffer.finished && (!buffer.truncated || buffer.tail))
	text, invalid := buffer.decoder.decode(value[start:end], offset+uint64(start))
	return protocol.ProcessCapture{Text: text, Truncated: buffer.truncated, InvalidUtf8: invalid}
}

func utf8Start(value []byte) int {
	start := 0
	for start < len(value) && value[start]&0xc0 == 0x80 {
		start++
	}
	return start
}

func utf8End(value []byte) int {
	if len(value) == 0 {
		return 0
	}
	start := len(value) - 1
	for start > 0 && len(value)-start < 4 && value[start]&0xc0 == 0x80 {
		start--
	}
	if !utf8.FullRune(value[start:]) {
		return start
	}
	return len(value)
}

func decodeOutput(value []byte) (string, bool) {
	if utf8.Valid(value) {
		return string(value), false
	}
	var text strings.Builder
	text.Grow(len(value))
	for len(value) > 0 {
		if !utf8.FullRune(value) {
			text.WriteRune(utf8.RuneError)
			break
		}
		r, size := utf8.DecodeRune(value)
		if r == utf8.RuneError && size == 1 {
			text.WriteRune(utf8.RuneError)
		} else {
			text.Write(value[:size])
		}
		value = value[size:]
	}
	return text.String(), true
}

func pageEnd(value []byte, limit int, finished bool) int {
	end := min(len(value), limit)
	for end > 0 && end < len(value) && value[end]&0xc0 == 0x80 {
		end--
	}
	if end == 0 && len(value) > 0 {
		_, end = utf8.DecodeRune(value)
	}
	if !finished && end == len(value) {
		end = utf8End(value[:end])
	}
	return end
}
