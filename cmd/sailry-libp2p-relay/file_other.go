//go:build !unix

package main

import "os"

const relayFileReadFlags = os.O_RDONLY
