package files

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const artifactHTMLLimit = 512 << 10

// ReadArtifactHTML verifies immutable content after its metadata read lease ends.
func ReadArtifactHTML(ctx context.Context, profile string, artifact protocol.ArtifactSummary, digest string) (string, error) {
	if artifact.Size > artifactHTMLLimit || !validRevision(digest) {
		return "", invalid("invalid artifact revision metadata")
	}
	fs, err := core.OpenFiles(profile, core.ManualFiles)
	if err != nil {
		return "", err
	}
	defer fs.Close()
	path := fmt.Sprintf("artifacts/%s/%s/%d.html", artifact.Session.String(), artifact.ID.String(), artifact.Revision)
	file, err := fs.Open(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	data, err := readBounded(ctx, file, artifactHTMLLimit)
	if err != nil {
		return "", err
	}
	if uint64(len(data)) != artifact.Size || revision(data) != digest {
		return "", conflict("artifact content changed")
	}
	if !utf8.Valid(data) {
		return "", invalid("artifact content is not UTF-8")
	}
	return string(data), nil
}

// WriteArtifactHTML publishes a captured unreferenced revision. SQL metadata is
// committed by the caller only after this physical receipt is returned.
func WriteArtifactHTML(ctx context.Context, profile string, artifact protocol.ArtifactSummary, html string) (string, error) {
	if html == "" || len(html) > artifactHTMLLimit || !utf8.ValidString(html) || !nonnull(html) {
		return "", invalid("artifact HTML is empty or exceeds its limit")
	}
	fs, err := core.OpenFiles(profile, core.ManualFiles)
	if err != nil {
		return "", err
	}
	defer fs.Close()
	directory := fmt.Sprintf("artifacts/%s/%s", artifact.Session.String(), artifact.ID.String())
	if err = fs.CreatePrivateDirectories(directory); err != nil {
		return "", err
	}
	path := fmt.Sprintf("%s/%d.html", directory, artifact.Revision)
	var expected *string
	file, err := fs.Open(path)
	if err == nil {
		data, readErr := readBounded(ctx, file, artifactHTMLLimit)
		file.Close()
		if readErr != nil {
			return "", readErr
		}
		digest := revision(data)
		expected = &digest
	} else {
		var value *protocol.Fault
		if !errors.Is(err, os.ErrNotExist) && (!errors.As(err, &value) || value.Code != protocol.ErrorCodeNotFound) {
			return "", err
		}
	}
	target, err := fs.PrepareFileTarget(ctx, path, expected, artifactHTMLLimit)
	if err != nil {
		return "", err
	}
	digest := revision([]byte(html))
	_, err = target.Write(strings.NewReader(html), uint64(len(html)), digest)
	return digest, err
}
