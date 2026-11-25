package panel

import "strings"

func baseName(path string) string { return path[strings.LastIndex(path, "/")+1:] }

func parentDir(path string) string {
	if i := strings.LastIndex(path, "/"); i >= 0 {
		return path[:i]
	}
	return ""
}
