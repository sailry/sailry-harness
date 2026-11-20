package link

import (
	"crypto/ed25519"
	"crypto/rand"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Identity belongs to the exclusive profile owner.
type Identity struct{ seed [ed25519.SeedSize]byte }

const identityName = "link.key"

func OpenIdentity(directory string) (Identity, error) {
	identity, err := ReadIdentity(directory)
	if !errors.Is(err, os.ErrNotExist) {
		return identity, err
	}
	if _, err := rand.Read(identity.seed[:]); err != nil {
		return Identity{}, err
	}
	path := filepath.Join(directory, identityName)
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err == nil {
		_, writeErr := file.Write(identity.seed[:])
		if writeErr == nil {
			writeErr = file.Sync()
		}
		if err := errors.Join(writeErr, file.Close()); err != nil {
			return Identity{}, err
		}
		if runtime.GOOS != "windows" {
			parent, err := os.Open(directory)
			if err != nil {
				return Identity{}, err
			}
			err = errors.Join(parent.Sync(), parent.Close())
			if err != nil {
				return Identity{}, err
			}
		}
		return identity, nil
	}
	if !errors.Is(err, os.ErrExist) {
		return Identity{}, err
	}
	return ReadIdentity(directory)
}

// ReadIdentity loads the existing key without creating one.
func ReadIdentity(directory string) (Identity, error) {
	var identity Identity
	root, err := os.OpenRoot(directory)
	if err != nil {
		return Identity{}, err
	}
	defer root.Close()
	metadata, err := root.Lstat(identityName)
	if err != nil {
		return Identity{}, err
	}
	if !metadata.Mode().IsRegular() {
		return Identity{}, errors.New("identity must be a regular private file")
	}
	file, err := openIdentityKey(root)
	if err != nil {
		return Identity{}, err
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil {
		return Identity{}, err
	}
	if !opened.Mode().IsRegular() || !os.SameFile(metadata, opened) {
		return Identity{}, errors.New("identity file changed before it could be read")
	}
	if runtime.GOOS != "windows" && opened.Mode().Perm()&0o077 != 0 {
		return Identity{}, errors.New("identity file permissions must exclude group and other users")
	}
	data, err := io.ReadAll(io.LimitReader(file, ed25519.SeedSize+1))
	if err != nil {
		return Identity{}, err
	}
	if len(data) != ed25519.SeedSize {
		return Identity{}, errors.New("invalid identity file length; refusing identity replacement")
	}
	copy(identity.seed[:], data)
	return identity, nil
}

func (identity Identity) ID() protocol.NodeID {
	key := ed25519.NewKeyFromSeed(identity.seed[:])
	return protocol.NodeID(key[ed25519.SeedSize:])
}

func (identity Identity) SecretKey() [ed25519.SeedSize]byte { return identity.seed }
func (Identity) String() string                             { return "Identity([REDACTED])" }
func (Identity) GoString() string                           { return "Identity([REDACTED])" }
