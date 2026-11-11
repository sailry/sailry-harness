package fswatch

import (
	"errors"
	"path/filepath"
	"strings"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
)

// FSEvents through purego. Every stream runs on one serial dispatch queue and
// calls back with the paths that changed below its root.
var fsevents struct {
	once  sync.Once
	err   error
	queue uintptr
	// callbacks is the kCFTypeArrayCallBacks address.
	callbacks uintptr
	callback  uintptr

	text       func(allocator uintptr, text string, encoding uint32) uintptr
	array      func(allocator uintptr, values *uintptr, count int, callbacks uintptr) uintptr
	free       func(value uintptr)
	queueNew   func(label string, attributes uintptr) uintptr
	create     func(allocator, callback uintptr, context *streamContext, paths uintptr, since uint64, latency float64, flags uint32) uintptr
	setQueue   func(stream, queue uintptr)
	start      func(stream uintptr) bool
	stop       func(stream uintptr)
	invalidate func(stream uintptr)
	release    func(stream uintptr)

	mutex   sync.Mutex
	next    uintptr
	streams map[uintptr]*stream
}

type streamContext struct {
	version         int
	info            uintptr
	retain, release uintptr
	describe        uintptr
}

const (
	encodingUTF8 = 0x08000100
	sinceNow     = ^uint64(0)
	// latency coalesces the events of a burst into one callback.
	latency = 0.1

	createNoDefer    = 0x2
	createFileEvents = 0x10

	// The kernel or the client dropped events; everything below the path
	// may have changed.
	eventRescan = 0x1 | 0x2 | 0x4
	// History markers carry no change.
	eventIDsWrapped  = 0x8
	eventHistoryDone = 0x10
)

func load() error {
	fsevents.once.Do(func() {
		foundation, err := purego.Dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			fsevents.err = err
			return
		}
		services, err := purego.Dlopen("/System/Library/Frameworks/CoreServices.framework/CoreServices", purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			fsevents.err = err
			return
		}
		system, err := purego.Dlopen("/usr/lib/libSystem.B.dylib", purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			fsevents.err = err
			return
		}
		if fsevents.callbacks, err = purego.Dlsym(foundation, "kCFTypeArrayCallBacks"); err != nil {
			fsevents.err = err
			return
		}
		purego.RegisterLibFunc(&fsevents.text, foundation, "CFStringCreateWithCString")
		purego.RegisterLibFunc(&fsevents.array, foundation, "CFArrayCreate")
		purego.RegisterLibFunc(&fsevents.free, foundation, "CFRelease")
		purego.RegisterLibFunc(&fsevents.queueNew, system, "dispatch_queue_create")
		purego.RegisterLibFunc(&fsevents.create, services, "FSEventStreamCreate")
		purego.RegisterLibFunc(&fsevents.setQueue, services, "FSEventStreamSetDispatchQueue")
		purego.RegisterLibFunc(&fsevents.start, services, "FSEventStreamStart")
		purego.RegisterLibFunc(&fsevents.stop, services, "FSEventStreamStop")
		purego.RegisterLibFunc(&fsevents.invalidate, services, "FSEventStreamInvalidate")
		purego.RegisterLibFunc(&fsevents.release, services, "FSEventStreamRelease")
		fsevents.queue = fsevents.queueNew("dev.sailry.fswatch", 0)
		fsevents.callback = purego.NewCallback(deliver)
		fsevents.streams = map[uintptr]*stream{}
	})
	return fsevents.err
}

// stream is one FSEvents stream. A stream opened by Dir reports only the
// directories added to it.
type stream struct {
	owner  *Watcher
	id     uintptr
	native uintptr
	// root is the caller's spelling and resolved the one FSEvents reports.
	root, resolved string
	dirs           map[string]bool
}

// Watcher delivers changed paths in batches. Events closes after Close.
type Watcher struct {
	Events chan []Event
	// Errors is never sent to on macOS; dropped events arrive as an event
	// that names the watched root.
	Errors chan error

	mutex   sync.Mutex
	streams map[string]*stream
	// owners maps a directory added with Dir to its stream's root.
	owners  map[string]string
	pending []Event
	closed  bool
	wake    chan struct{}
	done    chan struct{}
	pumped  chan struct{}
}

// New starts a watcher without watches.
func New() (*Watcher, error) {
	if err := load(); err != nil {
		return nil, err
	}
	watcher := &Watcher{
		Events: make(chan []Event), Errors: make(chan error),
		streams: map[string]*stream{}, owners: map[string]string{},
		wake: make(chan struct{}, 1), done: make(chan struct{}), pumped: make(chan struct{}),
	}
	go watcher.pump()
	return watcher, nil
}

// pump moves callback batches to the reader, so a slow reader never blocks
// the dispatch queue.
func (watcher *Watcher) pump() {
	defer close(watcher.pumped)
	defer close(watcher.Events)
	for {
		select {
		case <-watcher.done:
			return
		case <-watcher.wake:
		}
		watcher.mutex.Lock()
		batch := watcher.pending
		watcher.pending = nil
		watcher.mutex.Unlock()
		if len(batch) == 0 {
			continue
		}
		select {
		case <-watcher.done:
			return
		case watcher.Events <- batch:
		}
	}
}

func (watcher *Watcher) open(root string, dirs map[string]bool) (*stream, error) {
	resolved, err := filepath.EvalSymlinks(root)
	if err != nil {
		return nil, err
	}
	created := &stream{owner: watcher, root: root, resolved: resolved, dirs: dirs}
	fsevents.mutex.Lock()
	fsevents.next++
	created.id = fsevents.next
	fsevents.streams[created.id] = created
	fsevents.mutex.Unlock()
	path := fsevents.text(0, resolved, encodingUTF8)
	if path == 0 {
		created.forget()
		return nil, errors.New("file watch path is not valid text")
	}
	paths := fsevents.array(0, &path, 1, fsevents.callbacks)
	fsevents.free(path)
	context := streamContext{info: created.id}
	created.native = fsevents.create(0, fsevents.callback, &context, paths, sinceNow, latency, createNoDefer|createFileEvents)
	fsevents.free(paths)
	if created.native == 0 {
		created.forget()
		return nil, errors.New("file event stream was not created")
	}
	fsevents.setQueue(created.native, fsevents.queue)
	if !fsevents.start(created.native) {
		fsevents.invalidate(created.native)
		fsevents.release(created.native)
		created.forget()
		return nil, errors.New("file event stream did not start")
	}
	return created, nil
}

func (stream *stream) forget() {
	fsevents.mutex.Lock()
	delete(fsevents.streams, stream.id)
	fsevents.mutex.Unlock()
}

// end stops the stream. A callback that already started finds no stream.
func (stream *stream) end() {
	stream.forget()
	fsevents.stop(stream.native)
	fsevents.invalidate(stream.native)
	fsevents.release(stream.native)
}

// Tree watches root and everything below it, now and later.
func (watcher *Watcher) Tree(root string) error {
	watcher.mutex.Lock()
	defer watcher.mutex.Unlock()
	if watcher.closed {
		return errors.New("file watcher is closed")
	}
	if known := watcher.streams[root]; known != nil {
		// A tree covers the directories added below it one by one.
		known.dirs = nil
		return nil
	}
	created, err := watcher.open(root, nil)
	if err != nil {
		return err
	}
	watcher.streams[root] = created
	return nil
}

// Dir watches the entries of one directory. root is the directory tree that
// holds it; on macOS one stream on root serves every directory added under
// it.
func (watcher *Watcher) Dir(root, dir string) error {
	watcher.mutex.Lock()
	defer watcher.mutex.Unlock()
	if watcher.closed {
		return errors.New("file watcher is closed")
	}
	if _, known := watcher.owners[dir]; known {
		return nil
	}
	serving := watcher.streams[root]
	if serving == nil {
		created, err := watcher.open(root, map[string]bool{})
		if err != nil {
			return err
		}
		watcher.streams[root], serving = created, created
	}
	if serving.dirs != nil {
		serving.dirs[dir] = true
	}
	watcher.owners[dir] = root
	return nil
}

// Drop ends the watch of a directory added with Dir.
func (watcher *Watcher) Drop(dir string) {
	watcher.mutex.Lock()
	root, known := watcher.owners[dir]
	delete(watcher.owners, dir)
	var ended *stream
	if serving := watcher.streams[root]; known && serving != nil && serving.dirs != nil {
		delete(serving.dirs, dir)
		if len(serving.dirs) == 0 {
			delete(watcher.streams, root)
			ended = serving
		}
	}
	watcher.mutex.Unlock()
	if ended != nil {
		ended.end()
	}
}

// Close ends every watch and closes Events.
func (watcher *Watcher) Close() error {
	watcher.mutex.Lock()
	if watcher.closed {
		watcher.mutex.Unlock()
		return nil
	}
	watcher.closed = true
	streams := watcher.streams
	watcher.streams = map[string]*stream{}
	watcher.mutex.Unlock()
	for _, stream := range streams {
		stream.end()
	}
	close(watcher.done)
	<-watcher.pumped
	return nil
}

// deliver runs on the dispatch queue for one batch of a stream.
func deliver(_, info, count uintptr, paths unsafe.Pointer, flags unsafe.Pointer, _ uintptr) {
	fsevents.mutex.Lock()
	stream := fsevents.streams[info]
	fsevents.mutex.Unlock()
	if stream == nil || count == 0 {
		return
	}
	names := unsafe.Slice((**byte)(paths), int(count))
	marks := unsafe.Slice((*uint32)(flags), int(count))
	events := make([]Event, 0, count)
	for position, name := range names {
		mark := marks[position]
		if mark&(eventIDsWrapped|eventHistoryDone) != 0 {
			continue
		}
		if mark&eventRescan != 0 {
			events = []Event{{Path: stream.root}}
			break
		}
		events = append(events, Event{Path: stream.spelled(text(name))})
	}
	stream.owner.push(stream, events)
}

func text(start *byte) string {
	length := 0
	for *(*byte)(unsafe.Add(unsafe.Pointer(start), length)) != 0 {
		length++
	}
	return string(unsafe.Slice(start, length))
}

// spelled maps a reported path back below the root as the caller named it.
func (stream *stream) spelled(path string) string {
	// The volume may fold case, so the reported spelling can differ.
	if size := len(stream.resolved); len(path) >= size && strings.EqualFold(path[:size], stream.resolved) {
		if len(path) == size {
			return stream.root
		}
		if path[size] == filepath.Separator {
			return filepath.Join(stream.root, path[size+1:])
		}
	}
	return path
}

func (watcher *Watcher) push(stream *stream, events []Event) {
	watcher.mutex.Lock()
	defer watcher.mutex.Unlock()
	if watcher.closed || watcher.streams[stream.root] != stream {
		return
	}
	rescan := len(events) == 1 && events[0].Path == stream.root
	for _, event := range events {
		if stream.dirs != nil && !rescan && !stream.dirs[event.Path] && !stream.dirs[filepath.Dir(event.Path)] {
			continue
		}
		watcher.pending = append(watcher.pending, event)
	}
	if len(watcher.pending) > queued {
		// Keep one event per root: everything below it may have changed.
		watcher.pending = watcher.pending[:0]
		for root := range watcher.streams {
			watcher.pending = append(watcher.pending, Event{Path: root})
		}
	}
	select {
	case watcher.wake <- struct{}{}:
	default:
	}
}
