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

func OpenIdentity(directory string) (Identity, error) {
	var identity Identity
	if _, err := rand.Read(identity.seed[:]); err != nil {
		return Identity{}, err
	}
	path := filepath.Join(directory, "link.key")
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
	metadata, err := os.Lstat(path)
	if err != nil {
		return Identity{}, err
	}
	if !metadata.Mode().IsRegular() {
		return Identity{}, errors.New("identity must be a regular private file")
	}
	if runtime.GOOS != "windows" && metadata.Mode().Perm()&0o077 != 0 {
		return Identity{}, errors.New("identity file permissions must exclude group and other users")
	}
	file, err = os.Open(path)
	if err != nil {
		return Identity{}, err
	}
	defer file.Close()
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
