// Package assets acquires pinned model files: resumable downloads verified by
// size and SHA-256, and the file identities that prove a verified installation.
package assets

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"strings"
	"time"
)

var (
	// ErrStorage wraps every local file failure.
	ErrStorage = errors.New("model asset storage failed")
	// ErrDownload wraps every transport failure, without the request URL.
	ErrDownload = errors.New("model asset download failed")
	// ErrInvalidAsset reports a file that differs from its pinned identity.
	ErrInvalidAsset = errors.New("model asset validation failed")
	// ErrCancelled reports a cancelled acquisition.
	ErrCancelled = errors.New("model asset download cancelled")
)

func storage(err error) error { return fmt.Errorf("%w: %v", ErrStorage, err) }

func storageMessage(message string) error { return fmt.Errorf("%w: %s", ErrStorage, message) }

// download drops the request URL: redirect targets can carry temporary
// credentials and the public identity is already captured by the owner.
func download(err error) error {
	var located *url.Error
	if errors.As(err, &located) {
		err = located.Err
	}
	return fmt.Errorf("%w: %v", ErrDownload, err)
}

// File is one pinned file of a model directory.
type File struct {
	Name   string
	Size   uint64
	SHA256 string
}

// Download pairs a pinned file with its immutable HTTPS address.
type Download struct {
	File File
	URL  *url.URL
}

type Phase uint8

const (
	Inspecting Phase = iota
	Downloading
	Validating
	Installing
)

type Progress struct {
	Phase          Phase
	Percent        float32
	BytesPerSecond float64
}

func progress(phase Phase, completed, total, transferred uint64, elapsed time.Duration) Progress {
	seconds := max(elapsed.Seconds(), 0.001)
	// Cached bytes count toward completion, never toward network throughput.
	return Progress{
		Phase:          phase,
		Percent:        min(float32(completed)/float32(max(total, 1))*100, 99),
		BytesPerSecond: float64(transferred) / seconds,
	}
}

// Ready reports whether every file exists with its pinned size and SHA-256.
func Ready(directory string, files []File) bool {
	if _, err := validate(files); err != nil {
		return false
	}
	root, err := open(directory, false)
	if err != nil {
		return false
	}
	defer root.Close()
	for _, file := range files {
		if !valid(context.Background(), root, file) {
			return false
		}
	}
	return true
}

func validate(files []File) (uint64, error) {
	if len(files) == 0 {
		return 0, ErrInvalidAsset
	}
	names := map[string]bool{}
	var total uint64
	for _, file := range files {
		stem, _, _ := strings.Cut(file.Name, ".")
		stem = strings.ToUpper(stem)
		reserved := stem == "CON" || stem == "PRN" || stem == "AUX" || stem == "NUL"
		for _, prefix := range []string{"COM", "LPT"} {
			suffix, found := strings.CutPrefix(stem, prefix)
			reserved = reserved || found && len(suffix) == 1 && suffix[0] >= '1' && suffix[0] <= '9'
		}
		lower := strings.ToLower(file.Name)
		if file.Name == "" || len(file.Name) > 128 || strings.HasPrefix(file.Name, ".") || strings.HasSuffix(file.Name, ".") ||
			strings.ContainsFunc(file.Name, func(char rune) bool {
				return !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '.' || char == '_' || char == '-')
			}) || reserved || !hexadecimal(file.SHA256, 64) || names[lower] {
			return 0, ErrInvalidAsset
		}
		names[lower] = true
		if total+file.Size < total {
			return 0, ErrInvalidAsset
		}
		total += file.Size
	}
	return total, nil
}

func hexadecimal(value string, length int) bool {
	if len(value) != length {
		return false
	}
	for _, char := range []byte(value) {
		if !(char >= '0' && char <= '9' || char >= 'a' && char <= 'f' || char >= 'A' && char <= 'F') {
			return false
		}
	}
	return true
}
