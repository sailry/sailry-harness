package backup

import (
	"context"
	"errors"
	"io"
	"os"
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

// Seal replaces the packed archive with its password-encrypted age stream.
func Seal(ctx context.Context, packed *Packed, passphrase protocol.Secret) error {
	if err := cancelled(ctx); err != nil {
		return err
	}
	recipient, err := age.NewScryptRecipient(passphrase.Expose())
	if err != nil {
		return invalid("backup encryption could not be initialized")
	}
	recipient.SetWorkFactor(workFactor)
	input, err := os.Open(packed.Path)
	if err != nil {
		return ioFault(err)
	}
	defer input.Close()
	file, err := os.CreateTemp(packed.Snapshot.Directory, "sealed-")
	if err != nil {
		return ioFault(err)
	}
	defer file.Close()
	output, err := age.Encrypt(file, recipient)
	if err != nil {
		return ioFault(err)
	}
	if err := transfer(ctx, input, output); err != nil {
		return err
	}
	if err := errors.Join(output.Close(), file.Sync()); err != nil {
		return ioFault(err)
	}
	info, err := file.Stat()
	if err != nil {
		return ioFault(err)
	}
	if uint64(info.Size()) > MaxBytes {
		return invalid("encrypted backup exceeds the export limit")
	}
	sum, err := hashFile(ctx, file.Name(), uint64(info.Size()))
	if err != nil {
		return err
	}
	packed.Path, packed.Size, packed.SHA256 = file.Name(), uint64(info.Size()), sum
	return nil
}

// Open authenticates and decrypts an archive into a new file in temporary.
// The caller removes the returned file.
func Open(ctx context.Context, input io.Reader, temporary string, passphrase protocol.Secret) (*os.File, error) {
	if err := cancelled(ctx); err != nil {
		return nil, err
	}
	identity, err := age.NewScryptIdentity(passphrase.Expose())
	if err != nil {
		return nil, decryption()
	}
	identity.SetMaxWorkFactor(workFactor)
	reader, err := age.Decrypt(input, identity)
	if err != nil {
		return nil, decryption()
	}
	file, err := os.CreateTemp(temporary, "opened-")
	if err != nil {
		return nil, ioFault(err)
	}
	err = transfer(ctx, reader, file)
	if err == nil {
		err = wrapIO(file.Sync())
	}
	if err == nil {
		_, seekErr := file.Seek(0, io.SeekStart)
		err = wrapIO(seekErr)
	}
	if err != nil {
		_ = file.Close()
		_ = os.Remove(file.Name())
		var failure *protocol.Fault
		if errors.As(err, &failure) && failure.Code == protocol.ErrorCodeCancelled {
			return nil, err
		}
		return nil, decryption()
	}
	return file, nil
}

func transfer(ctx context.Context, input io.Reader, output io.Writer) error {
	buffer := make([]byte, 64*1024)
	var total uint64
	for {
		if err := cancelled(ctx); err != nil {
			return err
		}
		size, err := input.Read(buffer)
		if size > 0 {
			if total += uint64(size); total > MaxBytes {
				return invalid("backup exceeds the transfer limit")
			}
			if _, err := output.Write(buffer[:size]); err != nil {
				return ioFault(err)
			}
		}
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return ioFault(err)
		}
	}
}

func decryption() error {
	return invalid("backup could not be decrypted; check the passphrase and archive")
}
