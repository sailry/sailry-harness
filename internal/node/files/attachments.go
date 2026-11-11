package files

import (
	"context"
	"errors"
	"os"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func ValidateAttachment(spec protocol.AttachmentSpec) error {
	token := func(value string) bool {
		if value == "" {
			return false
		}
		for _, b := range []byte(value) {
			if b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z' || b >= '0' && b <= '9' || strings.ContainsRune("!#$&^_.+-", rune(b)) {
				continue
			}
			return false
		}
		return true
	}
	kind, subtype, ok := strings.Cut(spec.MediaType, "/")
	if spec.Name == "" || len(spec.Name) > 1024 || !utf8.ValidString(spec.Name) || strings.ContainsAny(spec.Name, "/\\") || strings.IndexFunc(spec.Name, unicode.IsControl) >= 0 || len(spec.MediaType) > 127 || !ok || !token(kind) || !token(subtype) || spec.Size > attachmentLimit {
		return invalid("invalid attachment metadata")
	}
	return nil
}
func (t *Transfers) attachmentDirectory(create bool) (*core.Files, error) {
	if t.profile == "" {
		return nil, transferUnavailable()
	}
	root, err := os.OpenRoot(t.profile)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	if create {
		if err = root.Mkdir("attachments", 0o700); err != nil && !errors.Is(err, os.ErrExist) {
			return nil, err
		}
	}
	info, err := root.Lstat("attachments")
	if err != nil {
		return nil, err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return nil, invalid("attachment storage is not an ordinary directory")
	}
	return core.OpenFiles(t.profile+string(os.PathSeparator)+"attachments", core.ManualFiles)
}
func (t *Transfers) OpenAttachment(attachment protocol.Attachment) (*os.File, error) {
	fs, err := t.attachmentDirectory(false)
	if err != nil {
		return nil, err
	}
	defer fs.Close()
	file, err := fs.Open(attachment.ID.String())
	if err != nil {
		return nil, err
	}
	info, err := file.Stat()
	if err != nil || uint64(info.Size()) != attachment.Spec.Size {
		file.Close()
		if err != nil {
			return nil, err
		}
		return nil, conflict("attachment content changed")
	}
	return file, nil
}
func (t *Transfers) AttachmentBytes(ctx context.Context, attachment protocol.Attachment) ([]byte, error) {
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	file, err := t.OpenAttachment(attachment)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	data, err := readBounded(ctx, file, attachmentLimit)
	if err != nil {
		return nil, err
	}
	if uint64(len(data)) != attachment.Spec.Size || revision(data) != attachment.Spec.Revision {
		return nil, conflict("attachment content changed")
	}
	return data, nil
}

// CollectAttachments removes every stored attachment file whose identifier is
// not retained. A finished upload publishes its file before its row commits,
// so the caller must exclude that window: it runs before admission, after the
// transfer workers drain, or under the lock that covers a finishing upload.
func (t *Transfers) CollectAttachments(retained map[protocol.AttachmentID]bool) error {
	fs, err := t.attachmentDirectory(false)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	defer fs.Close()

	var cursor *protocol.DirectoryCursor
	var unused []string
	for {
		page, err := fs.ListDirectory(context.Background(), "", cursor)
		if err != nil {
			return err
		}
		for _, entry := range page.Entries {
			if entry.Kind != protocol.EntryKindFile {
				continue
			}
			id, err := protocol.ParseAttachmentID(entry.Name)
			if err != nil || id.String() != entry.Name || retained[id] {
				continue
			}
			unused = append(unused, entry.Name)
		}
		if page.Next == nil {
			break
		}
		cursor = page.Next
	}
	for _, name := range unused {
		if err = fs.RemoveStoredFile(context.Background(), name); err != nil {
			return err
		}
	}
	return nil
}
