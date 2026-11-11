// Package fswatch reports file changes below directories without holding a
// descriptor per watched file.
//
// macOS uses one FSEvents stream per watched root. Other systems use
// fsnotify: inotify holds one watch per directory and Windows one handle per
// directory. fsnotify's kqueue backend is not used on macOS because it opens
// every file of a watched directory.
package fswatch

// Event names one changed path. Lost is set when the path was a directory
// added with Dir whose watch ended because it was removed or renamed; add it
// again once it exists.
type Event struct {
	Path string
	Lost bool
}

// queued bounds the events held for a slow reader. Beyond it the pending
// events of a root collapse into one event that names the root.
const queued = 4096
