// Package onnx is a minimal purego binding of the onnxruntime C API: the
// OrtApi function table reached through OrtGetApiBase, with CPU sessions
// only. The runtime is the pinned library packaged beside the executable.
package onnx

import (
	"errors"
	"fmt"
	"path/filepath"
	"sync"
	"unsafe"

	"github.com/ebitengine/purego"
)

// api holds the bound OrtApi functions and the process-wide runtime objects.
type api struct {
	getErrorMessage            func(status uintptr) *byte
	createEnv                  func(level int32, id string, env *uintptr) uintptr
	createSession              func(env uintptr, path *byte, options uintptr, session *uintptr) uintptr
	run                        func(session, runOptions uintptr, inputNames **byte, inputs *uintptr, inputCount uintptr, outputNames **byte, outputCount uintptr, outputs *uintptr) uintptr
	createSessionOptions       func(options *uintptr) uintptr
	setExecutionMode           func(options uintptr, mode int32) uintptr
	setIntraOpNumThreads       func(options uintptr, threads int32) uintptr
	setInterOpNumThreads       func(options uintptr, threads int32) uintptr
	addSessionConfigEntry      func(options uintptr, key, value string) uintptr
	sessionGetInputCount       func(session uintptr, count *uintptr) uintptr
	sessionGetOutputCount      func(session uintptr, count *uintptr) uintptr
	sessionGetInputTypeInfo    func(session, index uintptr, info *uintptr) uintptr
	sessionGetOutputTypeInfo   func(session, index uintptr, info *uintptr) uintptr
	sessionGetInputName        func(session, index, allocator uintptr, name **byte) uintptr
	sessionGetOutputName       func(session, index, allocator uintptr, name **byte) uintptr
	createTensorWithData       func(memory uintptr, data unsafe.Pointer, bytes uintptr, shape *int64, rank uintptr, element int32, value *uintptr) uintptr
	getTensorMutableData       func(value uintptr, data *unsafe.Pointer) uintptr
	castTypeInfoToTensorInfo   func(info uintptr, tensor *uintptr) uintptr
	getTensorElementType       func(tensor uintptr, element *int32) uintptr
	getDimensionsCount         func(tensor uintptr, count *uintptr) uintptr
	getDimensions              func(tensor uintptr, dimensions *int64, count uintptr) uintptr
	getTensorShapeElementCount func(tensor uintptr, count *uintptr) uintptr
	getTensorTypeAndShape      func(value uintptr, tensor *uintptr) uintptr
	createCPUMemoryInfo        func(allocatorType, memoryType int32, info *uintptr) uintptr
	allocatorFree              func(allocator uintptr, pointer *byte) uintptr
	getAllocatorWithDefaults   func(allocator *uintptr) uintptr
	releaseStatus              func(uintptr)
	releaseSession             func(uintptr)
	releaseValue               func(uintptr)
	releaseTypeInfo            func(uintptr)
	releaseTensorTypeAndShape  func(uintptr)
	releaseSessionOptions      func(uintptr)

	env, memory, allocator uintptr
	path                   string
}

var (
	mutex  sync.Mutex
	loaded *api
)

// Initialize loads the runtime packaged beside the executable. Every
// consumer shares the one library: a runtime that is already loaded is kept.
func Initialize() error {
	mutex.Lock()
	active := loaded != nil
	mutex.Unlock()
	if active {
		return nil
	}
	directory, err := Directory()
	if err != nil {
		return err
	}
	return Load(directory)
}

// Load opens the pinned runtime library in directory after checking every
// pinned runtime file against its size and SHA-256. The process keeps one
// runtime: a second directory is refused.
func Load(directory string) error {
	files, err := locked()
	if err != nil {
		return err
	}
	mutex.Lock()
	defer mutex.Unlock()
	if loaded != nil {
		if loaded.path == filepath.Join(directory, files[0].name) {
			return nil
		}
		return errors.New("ONNX Runtime is invalid: another runtime is already active")
	}
	path, err := verify(directory, files)
	if err != nil {
		return err
	}
	bound, err := open(path)
	if err != nil {
		return err
	}
	loaded = bound
	return nil
}

// open binds the verified library at path and creates the process environment.
func open(path string) (*api, error) {
	handle, err := openLibrary(path)
	if err != nil {
		return nil, fmt.Errorf("ONNX Runtime could not load: %w", err)
	}
	symbol, err := librarySymbol(handle, "OrtGetApiBase")
	if err != nil {
		return nil, fmt.Errorf("ONNX Runtime library is invalid: %w", err)
	}
	// struct OrtApiBase { GetApi; GetVersionString; }
	var getBase func() *[2]uintptr
	purego.RegisterFunc(&getBase, symbol)
	base := getBase()
	if base == nil {
		return nil, errors.New("ONNX Runtime is invalid: base API is unavailable")
	}
	var getAPI func(uint32) *[apiEntries]uintptr
	var getVersion func() string
	purego.RegisterFunc(&getAPI, base[0])
	purego.RegisterFunc(&getVersion, base[1])
	if getVersion() != Version {
		return nil, errors.New("ONNX Runtime is invalid: version must be " + Version)
	}
	table := getAPI(apiLevel)
	if table == nil {
		return nil, errors.New("ONNX Runtime is invalid: required API is unavailable")
	}
	bound := &api{path: path}
	for index, target := range map[int]any{
		apiGetErrorMessage:                &bound.getErrorMessage,
		apiCreateEnv:                      &bound.createEnv,
		apiCreateSession:                  &bound.createSession,
		apiRun:                            &bound.run,
		apiCreateSessionOptions:           &bound.createSessionOptions,
		apiSetSessionExecutionMode:        &bound.setExecutionMode,
		apiSetIntraOpNumThreads:           &bound.setIntraOpNumThreads,
		apiSetInterOpNumThreads:           &bound.setInterOpNumThreads,
		apiAddSessionConfigEntry:          &bound.addSessionConfigEntry,
		apiSessionGetInputCount:           &bound.sessionGetInputCount,
		apiSessionGetOutputCount:          &bound.sessionGetOutputCount,
		apiSessionGetInputTypeInfo:        &bound.sessionGetInputTypeInfo,
		apiSessionGetOutputTypeInfo:       &bound.sessionGetOutputTypeInfo,
		apiSessionGetInputName:            &bound.sessionGetInputName,
		apiSessionGetOutputName:           &bound.sessionGetOutputName,
		apiCreateTensorWithDataAsOrtValue: &bound.createTensorWithData,
		apiGetTensorMutableData:           &bound.getTensorMutableData,
		apiCastTypeInfoToTensorInfo:       &bound.castTypeInfoToTensorInfo,
		apiGetTensorElementType:           &bound.getTensorElementType,
		apiGetDimensionsCount:             &bound.getDimensionsCount,
		apiGetDimensions:                  &bound.getDimensions,
		apiGetTensorShapeElementCount:     &bound.getTensorShapeElementCount,
		apiGetTensorTypeAndShape:          &bound.getTensorTypeAndShape,
		apiCreateCpuMemoryInfo:            &bound.createCPUMemoryInfo,
		apiAllocatorFree:                  &bound.allocatorFree,
		apiGetAllocatorWithDefaultOptions: &bound.getAllocatorWithDefaults,
		apiReleaseStatus:                  &bound.releaseStatus,
		apiReleaseSession:                 &bound.releaseSession,
		apiReleaseValue:                   &bound.releaseValue,
		apiReleaseTypeInfo:                &bound.releaseTypeInfo,
		apiReleaseTensorTypeAndShapeInfo:  &bound.releaseTensorTypeAndShape,
		apiReleaseSessionOptions:          &bound.releaseSessionOptions,
	} {
		purego.RegisterFunc(target, table[index])
	}
	const loggingError = 3
	if err := bound.check(bound.createEnv(loggingError, "Sailry", &bound.env)); err != nil {
		return nil, err
	}
	// OrtArenaAllocator = 1, OrtMemTypeDefault = 0.
	if err := bound.check(bound.createCPUMemoryInfo(1, 0, &bound.memory)); err != nil {
		return nil, err
	}
	if err := bound.check(bound.getAllocatorWithDefaults(&bound.allocator)); err != nil {
		return nil, err
	}
	return bound, nil
}

// check converts and releases an OrtStatus.
func (bound *api) check(status uintptr) error {
	if status == 0 {
		return nil
	}
	message := goString(bound.getErrorMessage(status))
	bound.releaseStatus(status)
	return errors.New(message)
}

func goString(pointer *byte) string {
	if pointer == nil {
		return ""
	}
	length := 0
	for *(*byte)(unsafe.Add(unsafe.Pointer(pointer), length)) != 0 {
		length++
	}
	return string(unsafe.Slice(pointer, length))
}

func cString(value string) *byte {
	bytes := make([]byte, len(value)+1)
	copy(bytes, value)
	return &bytes[0]
}
