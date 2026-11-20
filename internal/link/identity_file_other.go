//go:build !unix

package link

import "os"

func openIdentityKey(root *os.Root) (*os.File, error) {
	return root.Open(identityName)
}
