//go:build unix && !linux && !darwin

package node

import "errors"

func renameLegacyProfile(string, string) error {
	return errors.New("atomic no-replace legacy profile migration is unsupported on this platform")
}
