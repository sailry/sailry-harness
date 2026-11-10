package source

import (
	"context"
	"encoding/hex"
	"io"
	"math"
	"os"
	"path/filepath"
	"strings"

	"lukechampine.com/blake3"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/localmodels/ocr"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// unavailable reports a file that could not be read as a transient failure,
// so that it interrupts the pass instead of being skipped.
func unavailable(err error) error {
	err = ioFault(err)
	if code(err) == protocol.ErrorCodeInvalidRequest {
		err.(*protocol.Fault).Code = protocol.ErrorCodeUnavailable
	}
	return err
}

func same(left, right os.FileInfo) bool {
	return left.Size() == right.Size() && left.ModTime().Equal(right.ModTime())
}

// Lines counts lines as a terminator-separated sequence.
func Lines(text string) []string {
	if text == "" {
		return nil
	}
	lines := strings.Split(strings.TrimSuffix(text, "\n"), "\n")
	for index, line := range lines {
		lines[index] = strings.TrimSuffix(line, "\r")
	}
	return lines
}

func read(ctx context.Context, file *target, path string, reuse func(string, Stat) (bool, error), reader *OCR) (*Entry, error) {
	if err := cancelled(ctx); err != nil {
		return nil, err
	}
	listed, err := file.directory.Lstat(file.name)
	if err != nil {
		return nil, ioFault(err)
	}
	if !listed.Mode().IsRegular() {
		return nil, invalid("expected a regular file")
	}
	opened, err := file.directory.Open(file.name)
	if err != nil {
		return nil, ioFault(err)
	}
	defer opened.Close()
	before, err := opened.Stat()
	if err != nil {
		return nil, unavailable(err)
	}
	if !os.SameFile(listed, before) {
		return nil, newFault(protocol.ErrorCodeRevisionConflict, "knowledge source changed during inspection")
	}
	if before.Size() > office.Limit {
		return nil, invalid("knowledge source exceeds 32 MiB")
	}
	modified := before.ModTime()
	if modified.Unix() < 0 {
		return nil, invalid("knowledge source modification time predates the epoch")
	}
	extension := strings.ToLower(strings.TrimPrefix(filepath.Ext(file.name), "."))
	image := ocr.Supported(file.name)
	stat := Stat{Size: uint64(before.Size()), ModifiedSeconds: uint64(modified.Unix()), ModifiedNanos: uint32(modified.Nanosecond())}
	if reader != nil && (extension == "pdf" || image) {
		if stat.Extractor, err = reader.Signature(); err != nil {
			return nil, err
		}
	}
	cached, err := reuse(path, stat)
	if err != nil {
		return nil, err
	}
	if cached {
		if err := cancelled(ctx); err != nil {
			return nil, err
		}
		after, err := opened.Stat()
		if err != nil {
			return nil, unavailable(err)
		}
		if !same(before, after) {
			return nil, newFault(protocol.ErrorCodeRevisionConflict, "knowledge source changed during inspection")
		}
		return &Entry{Path: path, Cached: true}, nil
	}
	data := make([]byte, 0, before.Size())
	buffer := make([]byte, 64*1024)
	for {
		if err := cancelled(ctx); err != nil {
			return nil, err
		}
		count, err := opened.Read(buffer)
		if len(data)+count > office.Limit {
			return nil, invalid("knowledge source exceeds 32 MiB")
		}
		data = append(data, buffer[:count]...)
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, unavailable(err)
		}
	}
	after, err := opened.Stat()
	if err != nil {
		return nil, unavailable(err)
	}
	if int64(len(data)) != before.Size() || !same(before, after) {
		return nil, newFault(protocol.ErrorCodeRevisionConflict, "knowledge source changed during extraction")
	}
	var parts []Part
	switch {
	case image && stat.Extractor == [32]byte{}:
		return excluded(path, "OCR is unavailable"), nil
	case image || extension == "pdf" || extension == "docx" || extension == "xlsx" || extension == "pptx":
		var extracted []office.Part
		if reader != nil {
			extracted, err = reader.Parts(ctx, file.name, data)
		} else {
			extracted, err = office.Parts(file.name, data)
		}
		if err != nil {
			return nil, err
		}
		for _, part := range extracted {
			parts = append(parts, Part{Title: part.Name, Text: part.Text, Position: part.Position})
		}
	default:
		text, err := core.DecodeText(data)
		if err != nil {
			return nil, invalid(err.Error())
		}
		lines := max(len(Lines(text)), 1)
		if lines > math.MaxUint32 {
			return nil, invalid("knowledge source has too many lines")
		}
		parts = []Part{{Title: file.name, Text: text, Position: protocol.KnowledgePosition{
			Kind: "lines", Data: protocol.KnowledgePositionLinesPayload{Start: 1, End: uint32(lines)},
		}}}
	}
	if err := cancelled(ctx); err != nil {
		return nil, err
	}
	hash := blake3.New(32, nil)
	hash.Write(data)
	if stat.Extractor != [32]byte{} {
		hash.Write(stat.Extractor[:])
	}
	return &Entry{Document: &Document{
		Path:       path,
		Revision:   hex.EncodeToString(hash.Sum(nil)),
		ModifiedMs: uint64(modified.UnixMilli()),
		Stat:       stat,
		Parts:      parts,
	}}, nil
}
