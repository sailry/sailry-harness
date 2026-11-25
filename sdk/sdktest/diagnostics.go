package sdktest

import (
	workspacegit "github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/node/host"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/telemetry"
)

type GitOptions = workspacegit.Options

type GitCommandResult = workspacegit.CommandResult

type TelemetryOptions = telemetry.Options

const (
	LogBegin     = logging.Begin
	LogAdmitted  = logging.Admitted
	LogCompleted = logging.Completed
	LogFailed    = logging.Failed
	LogCancelled = logging.Cancelled
)

func InstallLogging(profile, application string) (func(), error) {
	return logging.Install(profile, application)
}

func CurrentLogFile() (string, error) { return logging.CurrentFile() }

func LogTail(path string) ([]byte, error) { return logging.Tail(path) }

func ValidReportRef(value string) bool { return logging.ValidReportRef(value) }

func SetHostArtifactRoot(path string) func() {
	previous := host.ArtifactRoot
	host.ArtifactRoot = path
	return func() { host.ArtifactRoot = previous }
}
