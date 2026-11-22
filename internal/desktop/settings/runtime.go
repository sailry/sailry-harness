package settings

// PythonPreparation is a package whose Python dependencies are being
// prepared: Stage is creating, installing or validating.
type PythonPreparation struct{ Package, Stage string }
