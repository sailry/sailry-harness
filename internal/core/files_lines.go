package core

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"os"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func (files *Files) ReadLines(ctx context.Context, relative string, offset, limit uint64, expected *string) (protocol.FileLines, error) {
	if limit == 0 {
		return protocol.FileLines{}, fileFault(protocol.ErrorCodeInvalidRequest, "file line limit must be positive")
	}
	control := newFileControl(ctx)
	if err := control.check(); err != nil {
		return protocol.FileLines{}, err
	}
	file, err := files.Open(relative)
	if err != nil {
		return protocol.FileLines{}, err
	}
	defer file.Close()
	before, err := file.Stat()
	if err != nil {
		return protocol.FileLines{}, fileIOFault(err)
	}
	page := protocol.FileLines{Path: relative, Offset: offset, Lines: []protocol.FileLine{}}
	hash := blake3.New(32, nil)
	var buffer [8192]byte
	pending, retained := 0, 0
	var total int64
	var index uint64
	line := ""
	collecting, skippingLine := true, false
	for {
		if err := control.check(); err != nil {
			return protocol.FileLines{}, err
		}
		count, readErr := file.Read(buffer[pending:])
		if count == 0 {
			if readErr != nil && readErr != io.EOF {
				return protocol.FileLines{}, fileIOFault(readErr)
			}
			if pending != 0 {
				return protocol.FileLines{}, fileFault(protocol.ErrorCodeInvalidRequest, "file is not UTF-8 text")
			}
			break
		}
		total += int64(count)
		if total > before.Size() {
			return protocol.FileLines{}, fileFault(protocol.ErrorCodeRevisionConflict, "file changed during inspection")
		}
		hash.Write(buffer[pending : pending+count])
		chunk := buffer[:pending+count]
		if bytes.IndexByte(chunk, 0) >= 0 {
			return protocol.FileLines{}, fileFault(protocol.ErrorCodeInvalidRequest, "binary file cannot be displayed as text")
		}
		valid := len(chunk)
		for position := 0; position < len(chunk); {
			character, size := utf8.DecodeRune(chunk[position:])
			if character == utf8.RuneError && size == 1 {
				if !utf8.FullRune(chunk[position:]) {
					valid = position
					break
				}
				return protocol.FileLines{}, fileFault(protocol.ErrorCodeInvalidRequest, "file is not UTF-8 text")
			}
			position += size
		}
		for _, fragment := range strings.SplitAfter(string(chunk[:valid]), "\n") {
			if fragment == "" {
				continue
			}
			if !collecting {
				if !page.Truncated || page.NextOffset != nil {
					break
				}
				if skippingLine {
					if strings.HasSuffix(fragment, "\n") {
						index++
						skippingLine = false
					}
				} else {
					next := index
					page.NextOffset = &next
				}
				continue
			}
			if index >= offset {
				if uint64(len(page.Lines)) == min(limit, uint64(protocol.MAX_FILE_LINES)) {
					next := index
					page.NextOffset = &next
					collecting = false
					continue
				}
				if retained+len(line)+len(fragment) > protocol.MAX_FILE_BYTES {
					if len(page.Lines) == 0 {
						keep := protocol.MAX_FILE_BYTES - len(line)
						for keep > 0 && keep < len(fragment) && !utf8.RuneStart(fragment[keep]) {
							keep--
						}
						line += fragment[:keep]
						page.Lines = append(page.Lines, protocol.FileLine{Number: index + 1, Text: line})
						line = ""
						page.Truncated = true
						skippingLine = !strings.HasSuffix(fragment, "\n")
						if !skippingLine {
							index++
						}
					} else {
						next := index
						page.NextOffset = &next
						line = ""
					}
					collecting = false
					continue
				}
				line += fragment
			}
			if strings.HasSuffix(fragment, "\n") {
				if index >= offset {
					retained += len(line)
					page.Lines = append(page.Lines, protocol.FileLine{Number: index + 1, Text: line})
					line = ""
				}
				index++
			}
		}
		pending = copy(buffer[:], chunk[valid:])
	}
	after, err := file.Stat()
	if err != nil {
		return protocol.FileLines{}, fileIOFault(err)
	}
	if !fileMetadataUnchanged(before, after, total) {
		return protocol.FileLines{}, fileFault(protocol.ErrorCodeRevisionConflict, "file changed during inspection")
	}
	page.Size = uint64(total)
	page.Revision = fmt.Sprintf("%x", hash.Sum(nil))
	if expected != nil && *expected != page.Revision {
		return protocol.FileLines{}, fileFault(protocol.ErrorCodeRevisionConflict, "file changed before reading its indexed range")
	}
	if collecting && line != "" {
		page.Lines = append(page.Lines, protocol.FileLine{Number: index + 1, Text: line})
	}
	return page, control.check()
}

// BoundedText returns nil when the complete file exceeds the requested limit.
func (files *Files) BoundedText(ctx context.Context, relative string, limit int) (*string, error) {
	if limit == 0 || limit > protocol.MAX_EDIT_BYTES || limit < 0 {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, "text inspection limit is invalid")
	}
	control := newFileControl(ctx)
	if err := control.check(); err != nil {
		return nil, err
	}
	file, err := files.Open(relative)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	return boundedFileText(file, limit, control)
}

func boundedFileText(file *os.File, limit int, control fileControl) (*string, error) {
	before, err := file.Stat()
	if err != nil {
		return nil, fileIOFault(err)
	}
	if before.Size() > int64(limit) {
		return nil, nil
	}
	data, err := readFileBytes(file, limit+1, control)
	if err != nil {
		return nil, err
	}
	after, err := file.Stat()
	if err != nil {
		return nil, fileIOFault(err)
	}
	if len(data) > limit || !fileMetadataUnchanged(before, after, int64(len(data))) {
		return nil, fileFault(protocol.ErrorCodeRevisionConflict, "file changed during inspection")
	}
	if !utf8.Valid(data) {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, "file is not UTF-8 text")
	}
	if bytes.IndexByte(data, 0) >= 0 {
		return nil, fileFault(protocol.ErrorCodeInvalidRequest, "binary file cannot be displayed as text")
	}
	text := string(data)
	return &text, nil
}

func (files *Files) CompleteText(ctx context.Context, relative string) (string, error) {
	text, err := files.BoundedText(ctx, relative, protocol.MAX_EDIT_BYTES)
	if err != nil {
		return "", err
	}
	if text == nil {
		return "", fileFault(protocol.ErrorCodeInvalidRequest, "text file exceeds the inspection limit")
	}
	return *text, nil
}

func (files *Files) CaptureText(ctx context.Context, relative, expected string) (string, error) {
	text, err := files.CompleteText(ctx, relative)
	if err != nil {
		return "", err
	}
	if contentRevision([]byte(text)) != expected {
		return "", fileFault(protocol.ErrorCodeRevisionConflict, "file changed before checkpoint capture")
	}
	return text, nil
}
