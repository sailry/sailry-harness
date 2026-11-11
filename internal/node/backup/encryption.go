package backup

import (
	"context"
	"errors"
	"io"
	"strings"
	"unicode"

	"filippo.io/age"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// workFactor bounds memory use to approximately 256 MiB, including on small
// execution Nodes.
const workFactor = 18

// Passphrase accepts a stored or supplied passphrase for sealing or opening.
func Passphrase(value *protocol.Secret) (protocol.Secret, error) {
	if value == nil || value.Expose() == "" || len(value.Expose()) > 8192 || strings.ContainsFunc(value.Expose(), unicode.IsControl) {
		return protocol.Secret{}, fault(protocol.ErrorCodeNotConfigured, "backup passphrase is required")
	}
	return *value, nil
}

// Open authenticates the archive's header with the passphrase and returns the
// decrypted stream; nothing plain is written. The content is authenticated
// chunk by chunk as it is read, and a truncated stream fails at its end. A
// wrong passphrase is reported apart from an input that is not an archive.
func Open(ctx context.Context, input io.Reader, passphrase protocol.Secret) (io.Reader, error) {
	if err := cancelled(ctx); err != nil {
		return nil, err
	}
	identity, err := age.NewScryptIdentity(passphrase.Expose())
	if err != nil {
		return nil, invalid("backup passphrase is incorrect")
	}
	identity.SetMaxWorkFactor(workFactor)
	reader, err := age.Decrypt(input, identity)
	var mismatch *age.NoIdentityMatchError
	switch {
	case err == nil:
		return reader, nil
	case errors.As(err, &mismatch):
		return nil, invalid("backup passphrase is incorrect")
	}
	return nil, invalid("backup file is damaged or is not a Sailry backup")
}
