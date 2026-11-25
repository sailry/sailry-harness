package files

import "github.com/sailry/sailry-harness/internal/core"

type Files = core.Files

type FileAccess = core.FileAccess

type FileStamp = core.FileStamp

type FileListing = core.FileListing

type FoundFile = core.FoundFile

const RestrictedFiles = core.RestrictedFiles

func OpenFiles(directory string, access FileAccess) (*Files, error) {
	return core.OpenFiles(directory, access)
}

func PrivateDirectory(path string) error { return core.PrivateDirectory(path) }

func DatabaseFiles(path string) []string { return core.DatabaseFiles(path) }
