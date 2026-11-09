package core

import "github.com/Bios-Marcel/wastebasket/v2"

func trashNative(target string) (string, error) {
	return "", wastebasket.Trash(target)
}
