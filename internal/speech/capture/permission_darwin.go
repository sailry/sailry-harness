package capture

import (
	"context"
	"sync"

	"github.com/ebitengine/purego"
	"github.com/ebitengine/purego/objc"
)

// Core Audio returns silent buffers until macOS microphone access is
// granted, so dictation reads and requests the authorization itself.

var authorization struct {
	once    sync.Once
	err     error
	device  objc.ID
	media   objc.ID
	status  objc.SEL
	request objc.SEL
}

func avFoundation() error {
	a := &authorization
	a.once.Do(func() {
		if _, a.err = purego.Dlopen("/System/Library/Frameworks/AVFoundation.framework/AVFoundation", purego.RTLD_NOW|purego.RTLD_GLOBAL); a.err != nil {
			return
		}
		a.device = objc.ID(objc.GetClass("AVCaptureDevice"))
		// AVMediaTypeAudio is the constant string "soun"; this copy lives
		// for the process.
		a.media = objc.ID(objc.GetClass("NSString")).Send(objc.RegisterName("alloc")).Send(objc.RegisterName("initWithUTF8String:"), "soun\x00")
		a.status = objc.RegisterName("authorizationStatusForMediaType:")
		a.request = objc.RegisterName("requestAccessForMediaType:completionHandler:")
	})
	return a.err
}

// Status reads the microphone authorization without prompting.
func Status() Permission {
	if avFoundation() != nil || authorization.device == 0 || authorization.media == 0 {
		return PermissionUnknown
	}
	// AVAuthorizationStatus: 0 not determined, 1 restricted, 2 denied, 3 authorized.
	switch objc.Send[int](authorization.device, authorization.status, authorization.media) {
	case 0:
		return PermissionNotDetermined
	case 1:
		return PermissionRestricted
	case 2:
		return PermissionDenied
	case 3:
		return PermissionGranted
	}
	return PermissionUnknown
}

// Request raises the system's prompt when the user has not decided yet and
// reports whether capture is allowed. A decision already made is returned
// as it stands: only System Settings can change it.
func Request(ctx context.Context) (bool, error) {
	switch Status() {
	case PermissionGranted:
		return true, nil
	case PermissionNotDetermined:
		return ask(ctx)
	}
	return false, nil
}

// ask sends the request. AVFoundation copies the block and may invoke it on
// any queue; an authorization already decided answers without a prompt.
func ask(ctx context.Context) (bool, error) {
	if err := avFoundation(); err != nil {
		return false, err
	}
	answer := make(chan bool, 1)
	block := objc.NewBlock(func(_ objc.Block, granted bool) {
		select {
		case answer <- granted:
		default:
		}
	})
	authorization.device.Send(authorization.request, authorization.media, block)
	block.Release()
	select {
	case <-ctx.Done():
		return false, ctx.Err()
	case granted := <-answer:
		return granted, nil
	}
}
