package files

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// StageInputs creates verified command-local copies. Attachment IDs, never
// display names, identify the files in SAILRY_ATTACHMENTS.
func (t *Transfers) StageInputs(ctx context.Context, attachments []protocol.Attachment) (string, func(), error) {
	ctx, cancel := t.withTimeout(ctx, 30*time.Second)
	defer cancel()
	profile, err := os.OpenRoot(t.profile)
	if err != nil {
		return "", nil, err
	}
	defer profile.Close()
	if err = profile.Mkdir("tmp", 0o700); err != nil && !errors.Is(err, os.ErrExist) {
		return "", nil, err
	}
	metadata, err := profile.Lstat("tmp")
	if err != nil {
		return "", nil, err
	}
	if !metadata.IsDir() || metadata.Mode()&os.ModeSymlink != 0 {
		return "", nil, invalid("temporary directory is not an ordinary directory")
	}
	temporary, err := profile.OpenRoot("tmp")
	if err != nil {
		return "", nil, err
	}
	observed, err := temporary.Stat(".")
	if err != nil || !os.SameFile(metadata, observed) {
		temporary.Close()
		return "", nil, conflict("temporary directory changed")
	}
	name := "sailry-inputs-" + protocol.NewStreamID().String()
	if err = temporary.Mkdir(name, 0o700); err != nil {
		temporary.Close()
		return "", nil, err
	}
	directory, err := temporary.OpenRoot(name)
	if err != nil {
		_ = temporary.Remove(name)
		temporary.Close()
		return "", nil, err
	}
	identity, err := directory.Stat(".")
	if err != nil {
		directory.Close()
		_ = temporary.Remove(name)
		temporary.Close()
		return "", nil, err
	}
	var once sync.Once
	var owned []string
	release := func() {
		once.Do(func() {
			current, e := temporary.Lstat(name)
			if e == nil && os.SameFile(current, identity) {
				_ = directory.Chmod(".", 0o700)
				for _, file := range owned {
					_ = directory.Chmod(file, 0o600)
				}
				_ = temporary.RemoveAll(name)
			}
			_ = directory.Close()
			_ = temporary.Close()
		})
	}
	success := false
	defer func() {
		if !success {
			release()
		}
	}()
	buffer := make([]byte, protocol.FILE_TRANSFER_CHUNK_BYTES)
	for _, attachment := range attachments {
		if err = ctx.Err(); err != nil {
			return "", nil, err
		}
		if err = ValidateAttachment(attachment.Spec); err != nil {
			return "", nil, err
		}
		source, err := t.OpenAttachment(attachment)
		if err != nil {
			return "", nil, err
		}
		target, err := directory.OpenFile(attachment.ID.String(), os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
		if err != nil {
			source.Close()
			return "", nil, err
		}
		owned = append(owned, attachment.ID.String())
		digest := blake3.New(32, nil)
		var size uint64
		copyErr := func() error {
			for {
				if err := ctx.Err(); err != nil {
					return err
				}
				n, err := source.Read(buffer)
				if n > 0 {
					if uint64(n) > attachment.Spec.Size-size {
						return conflict("attachment content changed")
					}
					size += uint64(n)
					_, _ = digest.Write(buffer[:n])
					if err := writeBytes(target, buffer[:n]); err != nil {
						return err
					}
				}
				if errors.Is(err, io.EOF) {
					break
				}
				if err != nil {
					return err
				}
			}
			if size != attachment.Spec.Size || fmtRevision(digest.Sum(nil)) != attachment.Spec.Revision {
				return conflict("attachment content changed")
			}
			if err := target.Sync(); err != nil {
				return err
			}
			return target.Chmod(0o400)
		}()
		source.Close()
		closed := target.Close()
		if copyErr != nil {
			return "", nil, copyErr
		}
		if closed != nil {
			return "", nil, closed
		}
	}
	if err = ctx.Err(); err != nil {
		return "", nil, err
	}
	if err = directory.Chmod(".", 0o500); err != nil {
		return "", nil, err
	}
	success = true
	return filepath.Join(t.profile, "tmp", name), release, nil
}
