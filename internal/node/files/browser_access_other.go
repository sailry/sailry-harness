//go:build !unix

package files

func readable(string) bool { return true }
