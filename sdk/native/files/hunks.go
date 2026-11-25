package files

import "github.com/sailry/sailry-harness/internal/node/files"

type HunkChange = files.HunkChange

func SplitHunks(path, before, after string, segment int) ([]HunkChange, error) {
	return files.SplitHunks(path, before, after, segment)
}

func ApplyHunkText(current, from, to string) (string, error) {
	return files.ApplyHunkText(current, from, to)
}
