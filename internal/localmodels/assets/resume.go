package assets

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"hash"
	"io"
	"io/fs"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
)

var locks = struct {
	sync.Mutex
	held map[string]*lease
}{held: map[string]*lease{}}

type lease struct {
	slot  chan struct{}
	users int
}

// lock serializes transfers of one destination file in this process.
func lock(ctx context.Context, directory, name string) (func(), error) {
	resolved, err := filepath.EvalSymlinks(directory)
	if err != nil {
		return nil, storage(err)
	}
	key := filepath.Join(resolved, name)
	locks.Lock()
	current := locks.held[key]
	if current == nil {
		current = &lease{slot: make(chan struct{}, 1)}
		locks.held[key] = current
	}
	current.users++
	locks.Unlock()
	leave := func() {
		locks.Lock()
		if current.users--; current.users == 0 {
			delete(locks.held, key)
		}
		locks.Unlock()
	}
	select {
	case <-ctx.Done():
		leave()
		return nil, ErrCancelled
	case current.slot <- struct{}{}:
		return func() { <-current.slot; leave() }, nil
	}
}

// validator is the response identity a ranged request must still match.
type validator struct {
	Etag     *string `json:"Etag,omitempty"`
	Modified *string `json:"Modified,omitempty"`
}

func (value validator) text() string {
	if value.Etag != nil {
		return *value.Etag
	}
	return *value.Modified
}

func (value *validator) valid() bool {
	if value == nil || (value.Etag == nil) == (value.Modified == nil) {
		return false
	}
	text := value.text()
	if text == "" || len(text) > 1024 || strings.ContainsFunc(text, func(char rune) bool { return char < ' ' && char != '\t' || char == 0x7f }) {
		return false
	}
	return value.Etag == nil || strings.HasPrefix(text, `"`) && strings.HasSuffix(text, `"`)
}

func (value *validator) matches(response *http.Response) bool {
	if value == nil {
		return false
	}
	name := "Last-Modified"
	if value.Etag != nil {
		name = "ETag"
	}
	return response.Header.Get(name) == value.text()
}

type checkpoint struct {
	Version   uint8      `json:"version"`
	Endpoint  string     `json:"endpoint"`
	Received  uint64     `json:"received"`
	SHA256    string     `json:"sha256"`
	Validator *validator `json:"validator"`
}

// part is a private partial download derived from one immutable asset identity.
type part struct {
	file      *os.File
	received  uint64
	hash      hash.Hash
	root      *os.Root
	name      string
	marker    string
	endpoint  string
	validator *validator
}

func openPart(ctx context.Context, root *os.Root, download Download) (*part, error) {
	specification := download.File
	name := fmt.Sprintf(".%s-%d-%s.part", specification.Name, specification.Size, strings.ToLower(specification.SHA256))
	marker := name + ".json"
	if err := target(root, name); err != nil {
		return nil, err
	}
	if err := target(root, marker); err != nil {
		return nil, err
	}
	address := sha256.Sum256([]byte(download.URL.String()))
	endpoint := hex.EncodeToString(address[:])
	file, err := root.OpenFile(name, os.O_RDWR|os.O_CREATE, 0o600)
	if err != nil {
		return nil, storage(err)
	}
	info, err := file.Stat()
	if err != nil {
		file.Close()
		return nil, storage(err)
	}
	if !info.Mode().IsRegular() {
		file.Close()
		return nil, storageMessage("model asset checkpoint is not a regular file")
	}
	saved := readCheckpoint(root, marker)
	if saved != nil && (saved.Version != 1 || saved.Endpoint != endpoint || saved.Received > specification.Size || saved.Received > uint64(info.Size()) || !saved.Validator.valid()) {
		saved = nil
	}
	var received uint64
	if saved != nil {
		received = saved.Received
	}
	digest := sha256.New()
	buffer := make([]byte, 64*1024)
	for remaining := received; remaining > 0; {
		if ctx.Err() != nil {
			file.Close()
			return nil, ErrCancelled
		}
		count, err := file.Read(buffer[:min(remaining, uint64(len(buffer)))])
		if count == 0 {
			file.Close()
			if err != nil && err != io.EOF {
				return nil, storage(err)
			}
			return nil, storageMessage("model asset checkpoint ended unexpectedly")
		}
		digest.Write(buffer[:count])
		remaining -= uint64(count)
	}
	value := &part{file: file, received: received, hash: digest, root: root, name: name, marker: marker, endpoint: endpoint}
	if saved != nil && hex.EncodeToString(digest.Sum(nil)) != saved.SHA256 {
		value.received, value.hash = 0, sha256.New()
	}
	if err := file.Truncate(int64(value.received)); err != nil {
		file.Close()
		return nil, storage(err)
	}
	if _, err := file.Seek(int64(value.received), io.SeekStart); err != nil {
		file.Close()
		return nil, storage(err)
	}
	if value.received > 0 {
		value.validator = saved.Validator
	}
	return value, nil
}

func readCheckpoint(root *os.Root, marker string) *checkpoint {
	if info, err := root.Lstat(marker); err != nil || !info.Mode().IsRegular() {
		return nil
	}
	file, err := root.Open(marker)
	if err != nil {
		return nil
	}
	defer file.Close()
	body, err := io.ReadAll(io.LimitReader(file, 4097))
	if err != nil || len(body) > 4096 {
		return nil
	}
	var value checkpoint
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&value) != nil {
		return nil
	}
	return &value
}

// resume is the If-Range validator of a ranged continuation.
func (value *part) resume() (string, bool) {
	if value.validator == nil || value.received == 0 {
		return "", false
	}
	return value.validator.text(), true
}

func (value *part) reset() error {
	if err := value.file.Truncate(0); err != nil {
		return storage(err)
	}
	if _, err := value.file.Seek(0, io.SeekStart); err != nil {
		return storage(err)
	}
	value.received, value.hash, value.validator = 0, sha256.New(), nil
	return nil
}

func (value *part) headers(response *http.Response) {
	value.validator = nil
	if tag := response.Header.Get("ETag"); tag != "" {
		if candidate := (&validator{Etag: &tag}); candidate.valid() {
			value.validator = candidate
			return
		}
	}
	if modified := response.Header.Get("Last-Modified"); modified != "" {
		if candidate := (&validator{Modified: &modified}); candidate.valid() {
			value.validator = candidate
		}
	}
}

func (value *part) digest() string { return hex.EncodeToString(value.hash.Sum(nil)) }

func (value *part) complete(specification File) bool {
	return value.received == specification.Size && strings.EqualFold(value.digest(), specification.SHA256)
}

func (value *part) checkpoint() error {
	body, err := json.Marshal(checkpoint{Version: 1, Endpoint: value.endpoint, Received: value.received, SHA256: value.digest(), Validator: value.validator})
	if err != nil {
		return storage(err)
	}
	return replace(value.root, value.marker, body)
}

func (value *part) publish(name string) error {
	if err := value.file.Close(); err != nil {
		return storage(err)
	}
	if err := target(value.root, name); err != nil {
		return err
	}
	if err := value.root.Rename(value.name, name); err != nil {
		return storage(err)
	}
	if err := value.root.Remove(value.marker); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return storage(err)
	}
	return nil
}

func (value *part) discard() error {
	_ = value.file.Close()
	for _, name := range []string{value.name, value.marker} {
		if err := value.root.Remove(name); err != nil && !errors.Is(err, fs.ErrNotExist) {
			return storage(err)
		}
	}
	return nil
}

// ranged reports a 206 response that continues exactly at start through the
// end of the pinned size.
func ranged(response *http.Response, start, size uint64) bool {
	if start >= size {
		return false
	}
	value, found := strings.CutPrefix(response.Header.Get("Content-Range"), "bytes ")
	if !found {
		return false
	}
	span, total, found := strings.Cut(value, "/")
	if !found {
		return false
	}
	first, last, found := strings.Cut(span, "-")
	if !found {
		return false
	}
	from, fromErr := strconv.ParseUint(first, 10, 64)
	through, throughErr := strconv.ParseUint(last, 10, 64)
	whole, wholeErr := strconv.ParseUint(total, 10, 64)
	return fromErr == nil && throughErr == nil && wholeErr == nil && from == start && through+1 == size && whole == size &&
		(response.ContentLength < 0 || uint64(response.ContentLength) == size-start)
}
