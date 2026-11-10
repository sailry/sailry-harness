package python

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
)

const (
	readyMarker = ".sailry-ready.json"
	maxFiles    = 100000
	maxBytes    = 1024 * 1024 * 1024
)

type File struct {
	Path   string  `json:"path"`
	Size   uint64  `json:"size"`
	SHA256 string  `json:"sha256"`
	Link   *string `json:"link,omitempty"`
}

type environmentReceipt struct {
	Version uint32 `json:"version"`
	Key     string `json:"key"`
	Files   []File `json:"files"`
}

type stamp struct {
	size     int64
	modified int64
	mode     fs.FileMode
	identity string
}

type cachedHash struct {
	stamp  stamp
	digest string
}

type hashCache map[string]cachedHash

func fileStamp(info fs.FileInfo) stamp {
	value := reflect.ValueOf(info.Sys())
	if value.Kind() == reflect.Pointer {
		value = value.Elem()
	}
	var identity strings.Builder
	if value.Kind() == reflect.Struct {
		for _, name := range []string{"Dev", "Ino", "Ctim", "Ctimespec", "Ctime", "Ctimensec", "Birthtimespec", "CreationTime"} {
			field := value.FieldByName(name)
			if field.IsValid() && field.CanInterface() {
				_, _ = fmt.Fprint(&identity, name, "=", field.Interface(), ";")
			}
		}
	}
	return stamp{size: info.Size(), modified: info.ModTime().UnixNano(), mode: info.Mode(), identity: identity.String()}
}

func keyValid(value string) bool {
	if len(value) != 64 {
		return false
	}
	for _, character := range value {
		if !(character >= '0' && character <= '9' || character >= 'a' && character <= 'f') {
			return false
		}
	}
	return true
}

func within(root, path string) bool {
	relative, err := filepath.Rel(root, path)
	return err == nil && relative != ".." && !strings.HasPrefix(relative, ".."+string(filepath.Separator)) && !filepath.IsAbs(relative)
}

func hashFile(ctx context.Context, path string) (string, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", storage(err)
	}
	defer file.Close()
	before, err := file.Stat()
	if err != nil {
		return "", storage(err)
	}
	digest := sha256.New()
	buffer := make([]byte, 64*1024)
	var total int64
	for {
		if err := check(ctx); err != nil {
			return "", err
		}
		count, err := file.Read(buffer)
		if count > 0 {
			total += int64(count)
			if total > before.Size() {
				return "", invalid("Python environment changed during verification")
			}
			_, _ = digest.Write(buffer[:count])
		}
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return "", storage(err)
		}
	}
	after, err := file.Stat()
	if err != nil {
		return "", storage(err)
	}
	if total != before.Size() || fileStamp(before) != fileStamp(after) {
		return "", invalid("Python environment changed during verification")
	}
	return hex.EncodeToString(digest.Sum(nil)), nil
}

func inventory(ctx context.Context, root string) ([]string, error) {
	result := make([]string, 0)
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, failure error) error {
		if failure != nil {
			return storage(failure)
		}
		if err := check(ctx); err != nil {
			return err
		}
		if path == root {
			return nil
		}
		if entry.Name() == readyMarker || entry.Name() == ".sailry-used" {
			if entry.IsDir() {
				return invalid("Python environment marker must be a file")
			}
			return nil
		}
		if entry.IsDir() {
			return nil
		}
		if !entry.Type().IsRegular() && entry.Type()&os.ModeSymlink == 0 {
			return invalid("Python environment contains a special file")
		}
		if len(result) >= maxFiles {
			return invalid("Python environment contains too many files")
		}
		relative, err := filepath.Rel(root, path)
		if err != nil {
			return storage(err)
		}
		result = append(result, filepath.ToSlash(relative))
		return nil
	})
	slices.Sort(result)
	return result, err
}

func verifyFiles(ctx context.Context, root, runtime string, files []File, cache hashCache) (bool, error) {
	if len(files) == 0 || len(files) > maxFiles {
		return false, nil
	}
	seen := make(map[string]bool, len(files))
	var total uint64
	for _, file := range files {
		if err := check(ctx); err != nil {
			return false, err
		}
		if file.Path == "" || strings.Contains(file.Path, "\\") || !filepath.IsLocal(file.Path) || seen[file.Path] {
			return false, nil
		}
		for _, part := range strings.Split(file.Path, "/") {
			if part == "" || part == "." || part == ".." {
				return false, nil
			}
		}
		seen[file.Path] = true
		path := filepath.Join(root, filepath.FromSlash(file.Path))
		info, err := os.Lstat(path)
		if err != nil {
			return false, storage(err)
		}
		resolved, err := filepath.EvalSymlinks(path)
		if err != nil || !within(root, resolved) && !within(runtime, resolved) {
			return false, nil
		}
		if file.Link != nil {
			if info.Mode()&os.ModeSymlink == 0 {
				return false, nil
			}
			link, err := os.Readlink(path)
			if err != nil || link != *file.Link {
				return false, nil
			}
			continue
		}
		if info.Mode()&os.ModeSymlink != 0 {
			info, err = os.Stat(path)
			if err != nil {
				return false, storage(err)
			}
		}
		if !info.Mode().IsRegular() || info.Size() < 0 || uint64(info.Size()) != file.Size || !keyValid(file.SHA256) || file.Size > maxBytes-total {
			return false, nil
		}
		total += file.Size
		stamp := fileStamp(info)
		cached, exists := cache[path]
		digest := cached.digest
		if !exists || stamp != cached.stamp {
			digest, err = hashFile(ctx, path)
			if err != nil {
				return false, err
			}
			cache[path] = cachedHash{stamp: stamp, digest: digest}
		}
		if digest != file.SHA256 {
			return false, nil
		}
	}
	return true, nil
}

func seal(ctx context.Context, root, runtime, key string) error {
	paths, err := inventory(ctx, root)
	if err != nil {
		return err
	}
	files := make([]File, 0, len(paths))
	var total uint64
	for _, relative := range paths {
		path := filepath.Join(root, filepath.FromSlash(relative))
		info, err := os.Lstat(path)
		if err != nil {
			return storage(err)
		}
		file := File{Path: relative}
		if info.Mode()&os.ModeSymlink != 0 {
			resolved, err := filepath.EvalSymlinks(path)
			if err != nil || !within(root, resolved) && !within(runtime, resolved) {
				return invalid("Python environment link points outside its runtime")
			}
			link, err := os.Readlink(path)
			if err != nil {
				return storage(err)
			}
			file.Link = &link
		} else {
			if info.Size() < 0 || uint64(info.Size()) > maxBytes-total {
				return invalid("Python environment exceeds its size limit")
			}
			file.Size = uint64(info.Size())
			total += file.Size
			file.SHA256, err = hashFile(ctx, path)
			if err != nil {
				return err
			}
		}
		files = append(files, file)
	}
	body, err := json.Marshal(environmentReceipt{Version: 1, Key: key, Files: files})
	if err != nil {
		return storage(err)
	}
	return writePrivate(filepath.Join(root, readyMarker), body)
}

func validEnvironment(ctx context.Context, root, runtime, key string, cache hashCache) (bool, error) {
	if err := check(ctx); err != nil {
		return false, err
	}
	info, err := os.Lstat(root)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return false, nil
	}
	body, err := readReceipt(filepath.Join(root, readyMarker))
	if err != nil {
		return false, nil
	}
	var receipt environmentReceipt
	if decodeStrict(body, &receipt) != nil || receipt.Version != 1 || receipt.Key != key || !keyValid(key) {
		return false, nil
	}
	actual, err := inventory(ctx, root)
	if err != nil {
		if ctx.Err() != nil {
			return false, check(ctx)
		}
		return false, nil
	}
	listed := make([]string, 0, len(receipt.Files))
	for _, file := range receipt.Files {
		listed = append(listed, file.Path)
	}
	if !slices.Equal(actual, listed) {
		return false, nil
	}
	valid, err := verifyFiles(ctx, root, runtime, receipt.Files, cache)
	if err != nil && ctx.Err() == nil {
		return false, nil
	}
	return valid, err
}

func readReceipt(path string) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 || info.Size() > 16*1024*1024 {
		return nil, invalid("Python receipt is invalid")
	}
	body, err := os.ReadFile(path)
	if err != nil {
		return nil, storage(err)
	}
	return body, nil
}

func decodeStrict(body []byte, output any) error {
	decoder := json.NewDecoder(strings.NewReader(string(body)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(output); err != nil {
		return err
	}
	if err := decoder.Decode(new(any)); !errors.Is(err, io.EOF) {
		return invalid("Python receipt contains trailing data")
	}
	return nil
}

func writePrivate(path string, body []byte) error {
	if info, err := os.Lstat(path); err == nil {
		if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
			return invalid("Python environment file must be regular data")
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return storage(err)
	}
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o600)
	if err != nil {
		return storage(err)
	}
	_, writeErr := file.Write(body)
	return errors.Join(writeErr, file.Close())
}
