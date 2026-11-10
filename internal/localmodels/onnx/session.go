package onnx

import (
	"errors"
	"fmt"
	"runtime"
	"unsafe"
)

// Port describes one model input or output. Dynamic dimensions are -1.
type Port struct {
	Name    string
	Element int32
	Shape   []int64
}

// Session is one loaded model. It is not safe for concurrent use.
type Session struct {
	api     *api
	handle  uintptr
	Inputs  []Port
	Outputs []Port
	names   map[string]*byte
}

// Tensor is a dense input or output. Exactly one of Int64 and Float32 is set.
type Tensor struct {
	Name    string
	Shape   []int64
	Int64   []int64
	Float32 []float32
}

// Threads is the intra-op thread count of every session: half the CPUs,
// between one and four.
func Threads() int {
	return min(max(runtime.NumCPU()/2, 1), 4)
}

// NewSession opens the model at path for sequential CPU inference with
// bounded, non-spinning threads. Initialize or Load must have succeeded.
func NewSession(path string) (*Session, error) {
	mutex.Lock()
	bound := loaded
	mutex.Unlock()
	if bound == nil {
		return nil, errors.New("ONNX Runtime is not loaded")
	}
	var options uintptr
	if err := bound.check(bound.createSessionOptions(&options)); err != nil {
		return nil, err
	}
	defer bound.releaseSessionOptions(options)
	const sequential = 0
	for _, configure := range []func() uintptr{
		func() uintptr { return bound.setExecutionMode(options, sequential) },
		func() uintptr { return bound.setIntraOpNumThreads(options, int32(Threads())) },
		func() uintptr { return bound.setInterOpNumThreads(options, 1) },
		func() uintptr { return bound.addSessionConfigEntry(options, "session.intra_op.allow_spinning", "0") },
		func() uintptr { return bound.addSessionConfigEntry(options, "session.inter_op.allow_spinning", "0") },
	} {
		if err := bound.check(configure()); err != nil {
			return nil, err
		}
	}
	session := &Session{api: bound, names: map[string]*byte{}}
	if err := bound.check(bound.createSession(bound.env, sessionPath(path), options, &session.handle)); err != nil {
		return nil, err
	}
	var err error
	if session.Inputs, err = session.ports(bound.sessionGetInputCount, bound.sessionGetInputName, bound.sessionGetInputTypeInfo); err == nil {
		session.Outputs, err = session.ports(bound.sessionGetOutputCount, bound.sessionGetOutputName, bound.sessionGetOutputTypeInfo)
	}
	if err != nil {
		session.Close()
		return nil, err
	}
	return session, nil
}

func (session *Session) ports(
	count func(uintptr, *uintptr) uintptr,
	name func(uintptr, uintptr, uintptr, **byte) uintptr,
	info func(uintptr, uintptr, *uintptr) uintptr,
) ([]Port, error) {
	bound := session.api
	var total uintptr
	if err := bound.check(count(session.handle, &total)); err != nil {
		return nil, err
	}
	ports := make([]Port, total)
	for index := range ports {
		port := &ports[index]
		var pointer *byte
		if err := bound.check(name(session.handle, uintptr(index), bound.allocator, &pointer)); err != nil {
			return nil, err
		}
		port.Name = goString(pointer)
		if err := bound.check(bound.allocatorFree(bound.allocator, pointer)); err != nil {
			return nil, err
		}
		session.names[port.Name] = cString(port.Name)
		var typeInfo, tensor uintptr
		if err := bound.check(info(session.handle, uintptr(index), &typeInfo)); err != nil {
			return nil, err
		}
		// The tensor info is owned by the type info and is not released separately.
		err := bound.check(bound.castTypeInfoToTensorInfo(typeInfo, &tensor))
		if err == nil && tensor == 0 {
			err = errors.New("model port is not a tensor")
		}
		if err == nil {
			port.Element, port.Shape, err = bound.shape(tensor)
		}
		bound.releaseTypeInfo(typeInfo)
		if err != nil {
			return nil, err
		}
	}
	return ports, nil
}

func (bound *api) shape(tensor uintptr) (int32, []int64, error) {
	var element int32
	var rank uintptr
	if err := bound.check(bound.getTensorElementType(tensor, &element)); err != nil {
		return 0, nil, err
	}
	if err := bound.check(bound.getDimensionsCount(tensor, &rank)); err != nil {
		return 0, nil, err
	}
	shape := make([]int64, rank)
	if rank > 0 {
		if err := bound.check(bound.getDimensions(tensor, &shape[0], rank)); err != nil {
			return 0, nil, err
		}
	}
	return element, shape, nil
}

// Run evaluates every model output for the given inputs and returns copies
// of the float32 results in output order.
func (session *Session) Run(inputs []Tensor) ([]Tensor, error) {
	bound := session.api
	if len(inputs) == 0 || len(session.Outputs) == 0 {
		return nil, errors.New("model run requires an input and an output")
	}
	var pinner runtime.Pinner
	defer pinner.Unpin()
	inputNames := make([]*byte, len(inputs))
	values := make([]uintptr, len(inputs))
	outputs := make([]uintptr, len(session.Outputs))
	defer func() {
		for _, value := range append(values, outputs...) {
			if value != 0 {
				bound.releaseValue(value)
			}
		}
	}()
	for index, input := range inputs {
		name, known := session.names[input.Name]
		if !known {
			return nil, fmt.Errorf("unknown model input %q", input.Name)
		}
		inputNames[index] = name
		pinner.Pin(name)
		var data unsafe.Pointer
		var bytes uintptr
		var element int32
		switch {
		case len(input.Shape) == 0:
			return nil, errors.New("input tensor has no shape")
		case len(input.Int64) > 0:
			data, bytes, element = unsafe.Pointer(&input.Int64[0]), uintptr(len(input.Int64))*8, ElementInt64
		case len(input.Float32) > 0:
			data, bytes, element = unsafe.Pointer(&input.Float32[0]), uintptr(len(input.Float32))*4, ElementFloat32
		default:
			return nil, errors.New("input tensor is empty")
		}
		// The tensor borrows the Go buffers until it is released.
		pinner.Pin(data)
		pinner.Pin(&input.Shape[0])
		if err := bound.check(bound.createTensorWithData(bound.memory, data, bytes, &input.Shape[0], uintptr(len(input.Shape)), element, &values[index])); err != nil {
			return nil, err
		}
	}
	outputNames := make([]*byte, len(session.Outputs))
	for index, output := range session.Outputs {
		outputNames[index] = session.names[output.Name]
		pinner.Pin(outputNames[index])
	}
	pinner.Pin(&inputNames[0])
	pinner.Pin(&values[0])
	pinner.Pin(&outputNames[0])
	pinner.Pin(&outputs[0])
	if err := bound.check(bound.run(session.handle, 0, &inputNames[0], &values[0], uintptr(len(values)), &outputNames[0], uintptr(len(outputs)), &outputs[0])); err != nil {
		return nil, err
	}
	results := make([]Tensor, len(outputs))
	for index, value := range outputs {
		var tensor uintptr
		if err := bound.check(bound.getTensorTypeAndShape(value, &tensor)); err != nil {
			return nil, err
		}
		element, shape, err := bound.shape(tensor)
		var count uintptr
		if err == nil {
			err = bound.check(bound.getTensorShapeElementCount(tensor, &count))
		}
		bound.releaseTensorTypeAndShape(tensor)
		if err != nil {
			return nil, err
		}
		if element != ElementFloat32 {
			return nil, errors.New("output tensor is not float32")
		}
		result := Tensor{Name: session.Outputs[index].Name, Shape: shape, Float32: make([]float32, count)}
		if count > 0 {
			var data unsafe.Pointer
			if err := bound.check(bound.getTensorMutableData(value, &data)); err != nil {
				return nil, err
			}
			copy(result.Float32, unsafe.Slice((*float32)(data), count))
		}
		results[index] = result
	}
	return results, nil
}

// Close releases the model. The session must not be used afterwards.
func (session *Session) Close() {
	if session.handle != 0 {
		session.api.releaseSession(session.handle)
		session.handle = 0
	}
}
