//go:build unix

package main

import (
	"os"

	"golang.org/x/sys/unix"
)

const relayFileReadFlags = os.O_RDONLY | unix.O_NOFOLLOW | unix.O_NONBLOCK
