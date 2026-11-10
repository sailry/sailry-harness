package onnx

// Indices into the OrtApi function table, in the field order of ort-sys
// 2.0.0-rc.13. The table is append-only across onnxruntime releases, so the
// indices hold for every runtime that provides API level 17.
const (
	apiGetErrorMessage                = 2
	apiCreateEnv                      = 3
	apiCreateSession                  = 7
	apiRun                            = 9
	apiCreateSessionOptions           = 10
	apiSetSessionExecutionMode        = 13
	apiSetIntraOpNumThreads           = 24
	apiSetInterOpNumThreads           = 25
	apiSessionGetInputCount           = 30
	apiSessionGetOutputCount          = 31
	apiSessionGetInputTypeInfo        = 33
	apiSessionGetOutputTypeInfo       = 34
	apiSessionGetInputName            = 36
	apiSessionGetOutputName           = 37
	apiCreateTensorWithDataAsOrtValue = 49
	apiGetTensorMutableData           = 51
	apiCastTypeInfoToTensorInfo       = 55
	apiGetTensorElementType           = 60
	apiGetDimensionsCount             = 61
	apiGetDimensions                  = 62
	apiGetTensorShapeElementCount     = 64
	apiGetTensorTypeAndShape          = 65
	apiCreateCpuMemoryInfo            = 69
	apiAllocatorFree                  = 76
	apiGetAllocatorWithDefaultOptions = 78
	apiReleaseStatus                  = 93
	apiReleaseSession                 = 95
	apiReleaseValue                   = 96
	apiReleaseTypeInfo                = 98
	apiReleaseTensorTypeAndShapeInfo  = 99
	apiReleaseSessionOptions          = 100
	apiAddSessionConfigEntry          = 130

	// apiEntries is the length of the table prefix this binding reads.
	apiEntries = apiAddSessionConfigEntry + 1
)

// Version is the only onnxruntime release this binding accepts.
const Version = "1.28.2"

// apiLevel is the OrtApi revision requested from GetApi.
const apiLevel = 17

// Tensor element types of the onnxruntime C API.
const (
	ElementFloat32 int32 = 1
	ElementInt64   int32 = 7
)
