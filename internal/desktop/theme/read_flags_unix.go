//go:build unix

package theme

import (
	"golang.org/x/sys/unix"
	"os"
)

// A replacement during open must not follow a link or block a theme loader on a FIFO.
const themeReadFlags = os.O_RDONLY | unix.O_NONBLOCK | unix.O_NOFOLLOW
