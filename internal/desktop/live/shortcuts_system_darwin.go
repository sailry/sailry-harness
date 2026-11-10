package live

import (
	"runtime"
	"strings"
	"sync"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

// The front app's selected text through the Accessibility API, without
// touching its text or the clipboard.
type accessibility struct {
	trusted      func() bool
	application  func(pid int32) uintptr
	timeout      func(element uintptr, seconds float32) int32
	attribute    func(element, name uintptr, value *uintptr) int32
	elementType  func() uintptr
	release      func(uintptr)
	typeOf       func(uintptr) uintptr
	stringType   func() uintptr
	newString    func(allocator uintptr, text string, encoding uint32) uintptr
	stringLength func(uintptr) int64
	stringSize   func(length int64, encoding uint32) int64
	stringBytes  func(value uintptr, buffer *byte, size int64, encoding uint32) bool
}

const selectionUTF8 = 0x08000100

var loadAccessibility = sync.OnceValues(func() (*accessibility, error) {
	libraries := map[string]uintptr{}
	for _, name := range []string{"CoreFoundation", "ApplicationServices", "AppKit"} {
		library, err := purego.Dlopen("/System/Library/Frameworks/"+name+".framework/"+name, purego.RTLD_NOW|purego.RTLD_GLOBAL)
		if err != nil {
			return nil, err
		}
		libraries[name] = library
	}
	a := &accessibility{}
	for _, entry := range []struct {
		target        any
		library, name string
	}{
		{&a.trusted, "ApplicationServices", "AXIsProcessTrusted"}, {&a.application, "ApplicationServices", "AXUIElementCreateApplication"},
		{&a.timeout, "ApplicationServices", "AXUIElementSetMessagingTimeout"}, {&a.attribute, "ApplicationServices", "AXUIElementCopyAttributeValue"},
		{&a.elementType, "ApplicationServices", "AXUIElementGetTypeID"},
		{&a.release, "CoreFoundation", "CFRelease"}, {&a.typeOf, "CoreFoundation", "CFGetTypeID"}, {&a.stringType, "CoreFoundation", "CFStringGetTypeID"},
		{&a.newString, "CoreFoundation", "CFStringCreateWithCString"}, {&a.stringLength, "CoreFoundation", "CFStringGetLength"},
		{&a.stringSize, "CoreFoundation", "CFStringGetMaximumSizeForEncoding"}, {&a.stringBytes, "CoreFoundation", "CFStringGetCString"},
	} {
		address, err := purego.Dlsym(libraries[entry.library], entry.name)
		if err != nil {
			return nil, err
		}
		purego.RegisterFunc(entry.target, address)
	}
	return a, nil
})

// copy is an owned attribute of the wanted type, 0 without one.
func (a *accessibility) copy(element uintptr, name string, kind uintptr) uintptr {
	key := a.newString(0, name, selectionUTF8)
	if key == 0 {
		return 0
	}
	defer a.release(key)
	var value uintptr
	if a.attribute(element, key, &value) != 0 || value == 0 {
		return 0
	}
	if a.typeOf(value) != kind {
		a.release(value)
		return 0
	}
	return value
}

func (a *accessibility) text(element uintptr, name string) (string, bool) {
	value := a.copy(element, name, a.stringType())
	if value == 0 {
		return "", false
	}
	defer a.release(value)
	size := a.stringSize(a.stringLength(value), selectionUTF8) + 1
	if size <= 0 {
		return "", false
	}
	buffer := make([]byte, size)
	if !a.stringBytes(value, &buffer[0], size, selectionUTF8) {
		return "", false
	}
	// CFStringGetCString ends at the first NUL, so an embedded one shows as
	// a shorter text than the string's length.
	text, _, _ := strings.Cut(string(buffer), "\x00")
	return text, true
}

func readSelection() (string, string) {
	a, err := loadAccessibility()
	if err != nil {
		return "", "selection_unavailable"
	}
	if !a.trusted() {
		return "", "selection_permission"
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := objc.ID(objc.GetClass("NSAutoreleasePool")).Send(objc.RegisterName("new"))
	defer pool.Send(objc.RegisterName("drain"))
	front := objc.ID(objc.GetClass("NSWorkspace")).Send(objc.RegisterName("sharedWorkspace")).Send(objc.RegisterName("frontmostApplication"))
	if front == 0 {
		return "", "selection_empty"
	}
	element := a.application(objc.Send[int32](front, objc.RegisterName("processIdentifier")))
	if element == 0 {
		return "", "selection_unavailable"
	}
	a.timeout(element, 0.5)
	focused := a.copy(element, "AXFocusedUIElement", a.elementType())
	a.release(element)
	if focused == 0 {
		return "", "selection_empty"
	}
	defer a.release(focused)
	if role, _ := a.text(focused, "AXSubrole"); role == "AXSecureTextField" {
		return "", "selection_empty"
	}
	text, found := a.text(focused, "AXSelectedText")
	if !found {
		return "", "selection_empty"
	}
	return boundedSelection(text)
}
