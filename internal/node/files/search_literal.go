package files

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"io"
	"regexp"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// Bound retained match overlap and preceding context independently of file size.
	literalNeedleLimit  = 4 << 10
	literalContextLimit = 8
)

var errLiteralFallback = errors.New("text requires decoded search")

func canScanLiteral(input protocol.FileSearch) bool {
	if input.Regex || input.Multiline || len(input.Query) > literalNeedleLimit || strings.ContainsRune(input.Query, '\n') || !utf8.ValidString(input.Query) {
		return false
	}
	if input.Mode == protocol.SearchModeContent && input.Context > literalContextLimit {
		return false
	}
	return input.CaseSensitive || ascii(input.Query)
}

func ascii(text string) bool {
	for i := 0; i < len(text); i++ {
		if text[i] >= utf8.RuneSelf {
			return false
		}
	}
	return true
}

type literalSearch struct {
	input      io.LimitedReader
	reader     *bufio.Reader
	needle     []byte
	fold       bool
	matchWidth int // Maximum byte length of a match, including any wider folds.
	unicodeRE  *regexp.Regexp
	mode       protocol.SearchMode
	before     []literalContextLine
	folded     []byte
	joining    []byte
	prefix     []byte
}

type literalContextLine struct {
	text      []byte
	number    uint64
	truncated bool
}

func newLiteralSearch(input protocol.FileSearch) *literalSearch {
	s := &literalSearch{needle: []byte(input.Query), fold: !input.CaseSensitive, matchWidth: len(input.Query), mode: input.Mode}
	s.reader = bufio.NewReaderSize(&s.input, boundedReadChunkSize)
	if s.fold {
		// Only ASCII folded queries enter this scanner. A matching rune can
		// still be wider, for example Kelvin sign (U+212A) matching k.
		for i, b := range s.needle {
			s.needle[i] = lowerASCII(b)
			width := 1
			for r := unicode.SimpleFold(rune(b)); r != rune(b); r = unicode.SimpleFold(r) {
				width = max(width, utf8.RuneLen(r))
			}
			s.matchWidth += width - 1
		}
	}
	if input.Mode == protocol.SearchModeContent && input.Context > 0 {
		s.before = make([]literalContextLine, int(input.Context))
		storage := make([]byte, len(s.before)*searchLineByteLimit)
		for i := range s.before {
			s.before[i].text = storage[:0:searchLineByteLimit]
			storage = storage[searchLineByteLimit:]
		}
	}
	return s
}

// scan validates the entire file (including bytes after the first match) so a
// late binary marker or decoding error cannot turn into a partial text result.
// The reader and context slots are shared across files in the same query.
func (s *literalSearch) scan(ctx context.Context, input io.Reader, emit func([]byte, uint64, bool, bool)) (uint64, error) {
	s.input = io.LimitedReader{R: input, N: searchFileByteLimit + 1}
	s.reader.Reset(&s.input)
	var matches, number uint64
	var head, pending, after int
	for {
		line, readErr := s.readLine(ctx)
		if readErr != nil && readErr != io.EOF && readErr != bufio.ErrBufferFull {
			return 0, readErr
		}
		if number == 0 {
			line = bytes.TrimPrefix(line, []byte("\xef\xbb\xbf"))
		}
		if len(line) == 0 && readErr == io.EOF {
			return matches, nil
		}
		number++
		var matched, truncated bool
		needMatch := s.mode != protocol.SearchModeFiles || matches == 0
		if readErr == bufio.ErrBufferFull {
			var err error
			line, matched, truncated, err = s.longLine(ctx, line, needMatch)
			if err != nil {
				return 0, err
			}
		} else {
			line = trimSearchLine(line)
			nonASCIIEnd, err := validateLiteral(line)
			if err != nil {
				return 0, err
			}
			matched = needMatch && s.contains(line, nonASCIIEnd != 0)
			line, truncated = searchLinePrefix(line)
		}
		if matched {
			matches++
		}
		if s.mode != protocol.SearchModeContent {
			continue
		}
		switch {
		case matched:
			for i := range pending {
				row := s.before[(head+i)%len(s.before)]
				emit(row.text, row.number, false, row.truncated)
			}
			head, pending, after = 0, 0, len(s.before)
			emit(line, number, true, truncated)
		case after > 0:
			emit(line, number, false, truncated)
			after--
		case len(s.before) > 0:
			slot := &s.before[(head+pending)%len(s.before)]
			if pending == len(s.before) {
				head = (head + 1) % len(s.before)
			} else {
				pending++
			}
			slot.text = slot.text[:copy(slot.text[:cap(slot.text)], line)]
			slot.number, slot.truncated = number, truncated
		}
	}
}

func (s *literalSearch) readLine(ctx context.Context) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	line, err := s.reader.ReadSlice('\n')
	if s.input.N == 0 {
		return nil, invalid("file exceeds the inspection limit")
	}
	return line, err
}

// For a line larger than the reader buffer, carry only the bytes needed to
// match across reads. Leave its last byte and any incomplete rune unvalidated
// until the next read, when we can distinguish a line-ending CR from text.
func (s *literalSearch) longLine(ctx context.Context, part []byte, needMatch bool) ([]byte, bool, bool, error) {
	if s.joining == nil {
		s.joining = make([]byte, boundedReadChunkSize+s.matchWidth+utf8.UTFMax)
		s.prefix = make([]byte, searchLineByteLimit)
	}
	prefix, _ := searchLinePrefix(part)
	prefix = s.prefix[:copy(s.prefix[:], prefix)]
	var carry, validated, nonASCIIEnd int
	var matched bool
	for readErr := bufio.ErrBufferFull; ; {
		buf := s.joining[:carry+copy(s.joining[carry:], part)]
		end := len(buf)
		if readErr == bufio.ErrBufferFull {
			end--
			for end > validated && !utf8.RuneStart(buf[end]) {
				end--
				if len(buf)-end > utf8.UTFMax {
					return nil, false, false, errLiteralFallback
				}
			}
		} else {
			end = len(trimSearchLine(buf))
		}
		wideEnd, err := validateLiteral(buf[validated:end])
		if err != nil {
			return nil, false, false, err
		}
		if wideEnd != 0 {
			nonASCIIEnd = validated + wideEnd
		}
		matched = matched || needMatch && s.contains(buf[:end], nonASCIIEnd != 0)
		if readErr != bufio.ErrBufferFull {
			return prefix, matched, true, nil
		}
		keep := max(0, end-s.matchWidth+1)
		// No match can start inside a UTF-8 rune. Advancing to the next
		// boundary keeps every possible start and feeds complete runes to the matcher.
		for keep < end && !utf8.RuneStart(buf[keep]) {
			keep++
		}
		validated = end - keep
		// Only non-ASCII bytes still in the overlap affect the next match.
		nonASCIIEnd = max(0, nonASCIIEnd-keep)
		carry = copy(s.joining, buf[keep:])
		part, readErr = s.readLine(ctx)
		if readErr != nil && readErr != io.EOF && readErr != bufio.ErrBufferFull {
			return nil, false, false, readErr
		}
	}
}

// validateLiteral returns the end offset of the last non-ASCII rune, or zero.
func validateLiteral(line []byte) (nonASCIIEnd int, err error) {
	for i := 0; i < len(line); i++ {
		b := line[i]
		switch {
		case b < ' ' && b != '\t' && b != '\r' && b != '\f' || b == 0x7f:
			return 0, core.ErrBinaryText
		case b >= utf8.RuneSelf:
			r, width := utf8.DecodeRune(line[i:])
			// C1 bytes in non-UTF-8 files may decode differently as GB18030.
			if r == utf8.RuneError && width == 1 || r <= 0x9f {
				return 0, errLiteralFallback
			}
			nonASCIIEnd = i + width
			i += width - 1
		}
	}
	return nonASCIIEnd, nil
}

func (s *literalSearch) contains(line []byte, nonASCII bool) bool {
	if s.fold {
		if nonASCII && s.matchWidth > len(s.needle) {
			if s.unicodeRE == nil {
				// The bounded, quoted ASCII literal uses the same simple-fold
				// matching as the decoded path, without rereading the file.
				s.unicodeRE = regexp.MustCompile("(?i)" + regexp.QuoteMeta(string(s.needle)))
			}
			return s.unicodeRE.Match(line)
		}
		for i, b := range line {
			if lowerASCII(b) == b {
				continue
			}
			if s.folded == nil {
				s.folded = make([]byte, boundedReadChunkSize+s.matchWidth+utf8.UTFMax)
			}
			folded := s.folded[:len(line)]
			copy(folded, line[:i])
			for j, b := range line[i:] {
				folded[i+j] = lowerASCII(b)
			}
			return bytes.Contains(folded, s.needle)
		}
	}
	return bytes.Contains(line, s.needle)
}

func lowerASCII(b byte) byte {
	if 'A' <= b && b <= 'Z' {
		return b + ('a' - 'A')
	}
	return b
}

func trimSearchLine(line []byte) []byte {
	return bytes.TrimSuffix(bytes.TrimSuffix(line, []byte("\n")), []byte("\r"))
}

func searchLinePrefix(line []byte) ([]byte, bool) {
	if len(line) <= searchLineByteLimit {
		return line, false
	}
	end := searchLineByteLimit
	for end > 0 && !utf8.RuneStart(line[end]) {
		end--
	}
	return line[:end], true
}
