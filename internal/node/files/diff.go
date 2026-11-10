package files

import (
	"bytes"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/filemode"
	fdiff "github.com/go-git/go-git/v5/plumbing/format/diff"
	udiff "github.com/go-git/go-git/v5/utils/diff"
	"github.com/sergi/go-diff/diffmatchpatch"
	"strings"
)

type diffFile struct {
	path string
	hash plumbing.Hash
}

func (f diffFile) Path() string            { return f.path }
func (f diffFile) Hash() plumbing.Hash     { return f.hash }
func (f diffFile) Mode() filemode.FileMode { return filemode.Regular }

type diffChunk struct {
	text string
	kind fdiff.Operation
}

func (c diffChunk) Content() string       { return c.text }
func (c diffChunk) Type() fdiff.Operation { return c.kind }

type textPatch struct {
	from, to fdiff.File
	chunks   []fdiff.Chunk
}

func (p textPatch) IsBinary() bool                  { return false }
func (p textPatch) Files() (fdiff.File, fdiff.File) { return p.from, p.to }
func (p textPatch) Chunks() []fdiff.Chunk           { return p.chunks }

type diffPatch struct{ file textPatch }

func (p diffPatch) FilePatches() []fdiff.FilePatch { return []fdiff.FilePatch{p.file} }
func (p diffPatch) Message() string                { return "" }
func TextDiff(path, before, after string) (string, uint64, uint64) {
	p := textPatch{from: diffFile{path, plumbing.ComputeHash(plumbing.BlobObject, []byte(before))}, to: diffFile{path, plumbing.ComputeHash(plumbing.BlobObject, []byte(after))}}
	var added, removed uint64
	for _, d := range udiff.Do(before, after) {
		kind := fdiff.Equal
		count := uint64(strings.Count(d.Text, "\n"))
		if len(d.Text) > 0 && !strings.HasSuffix(d.Text, "\n") {
			count++
		}
		switch d.Type {
		case diffmatchpatch.DiffInsert:
			kind = fdiff.Add
			added += count
		case diffmatchpatch.DiffDelete:
			kind = fdiff.Delete
			removed += count
		}
		p.chunks = append(p.chunks, diffChunk{d.Text, kind})
	}
	var out bytes.Buffer
	_ = fdiff.NewUnifiedEncoder(&out, 3).Encode(diffPatch{p})
	return out.String(), added, removed
}
