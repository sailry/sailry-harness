//go:build unix

package link

import (
	"os"

	"golang.org/x/sys/unix"
)

func openIdentityKey(root *os.Root) (*os.File, error) {
	return root.OpenFile(identityName, os.O_RDONLY|unix.O_NOFOLLOW|unix.O_NONBLOCK, 0)
}
