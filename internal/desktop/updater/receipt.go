package updater

import (
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"log"
	"os"
	"path/filepath"
	"strings"

	"github.com/Masterminds/semver/v3"
)

// receipt is what an installation leaves for the next start.
type receipt struct {
	Version            uint32  `json:"version"`
	Result             string  `json:"result"`
	ApplicationVersion *string `json:"application_version,omitempty"`
	Message            *string `json:"message,omitempty"`
	Uncertain          *bool   `json:"uncertain,omitempty"`
	Installed          *bool   `json:"installed,omitempty"`
	Backup             *string `json:"backup,omitempty"`
}

// readReceipt acknowledges the previous installation's receipt: it is moved
// aside, then reported as the notice key of a completed update or as the
// failure it recorded. No receipt is no notice.
func readReceipt(directory, installed, running string) (string, error) {
	path := filepath.Join(directory, "receipt.json")
	file, err := os.Open(path)
	if errors.Is(err, fs.ErrNotExist) {
		return "", nil
	}
	if err != nil {
		return "", ioFailure(err)
	}
	defer file.Close()
	receipts := filepath.Join(directory, "receipts")
	if err := os.MkdirAll(receipts, 0o700); err != nil {
		return "", ioFailure(err)
	}
	archived, err := os.CreateTemp(receipts, "receipt-*.json")
	if err != nil {
		return "", ioFailure(err)
	}
	archived.Close()
	if err := os.Rename(path, archived.Name()); err != nil {
		_ = os.Remove(archived.Name())
		return "", ioFailure(err)
	}
	data, err := io.ReadAll(io.LimitReader(file, 64*1024+1))
	if err != nil {
		return "", ioFailure(err)
	}
	if len(data) > 64*1024 {
		return "", failure("updates_receipt_unread", "the update receipt exceeds its size limit")
	}
	var value receipt
	if err := json.Unmarshal(data, &value); err != nil {
		return "", failure("updates_receipt_unread", "the update receipt is invalid")
	}
	valid := value.Version == 1 && (value.Result == "success" || value.Result == "failure")
	if valid && value.Result == "success" {
		_, err := semver.StrictNewVersion(deref(value.ApplicationVersion, ""))
		valid = err == nil
	}
	if !valid {
		return "", failure("updates_receipt_unread", "the update receipt has an unsupported definition")
	}
	uncertain := value.Uncertain != nil && *value.Uncertain
	switch {
	case value.Result == "failure":
		key := "updates_install_failed"
		if value.Installed != nil && *value.Installed {
			key = "updates_restart_failed"
		} else if uncertain {
			key = "updates_install_uncertain"
		}
		return "", failure(key, deref(value.Message, "the prior update did not complete"))
	case uncertain:
		return "", failure("updates_install_uncertain", deref(value.Message, "the prior installation is unconfirmed"))
	case *value.ApplicationVersion != running:
		return "", failure("updates_restart_failed", "the recorded update does not match the running application")
	}
	if value.Backup != nil {
		cleanup(*value.Backup, installed)
	}
	return "updates_completed", nil
}

func deref(value *string, fallback string) string {
	if value == nil {
		return fallback
	}
	return *value
}

// cleanup deletes the retained original only when it is provably the backup
// this updater made beside the installed bundle.
func cleanup(backup, installed string) {
	directory := func(path string) bool {
		info, err := os.Lstat(path)
		return err == nil && info.IsDir()
	}
	if installed == "" || !filepath.IsAbs(backup) || !strings.HasPrefix(filepath.Base(backup), ".sailry-update-backup-") ||
		!directory(backup) || !directory(filepath.Join(backup, "Sailry.app")) {
		log.Printf("desktop update: backup kept unverified")
		return
	}
	resolved, err := filepath.EvalSymlinks(backup)
	current, currentErr := filepath.EvalSymlinks(installed)
	if err != nil || currentErr != nil || filepath.Dir(resolved) != filepath.Dir(current) || resolved == current {
		log.Printf("desktop update: backup kept unverified")
		return
	}
	if err := os.RemoveAll(resolved); err != nil {
		log.Printf("desktop update: backup could not be deleted: %v", err)
	}
}

func writeReceipt(path string, value receipt) error {
	data, err := json.Marshal(value)
	if err != nil {
		return failure("updates_io_failed", "update receipt serialization failed")
	}
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		return ioFailure(err)
	}
	defer file.Close()
	if _, err := file.Write(data); err != nil {
		return ioFailure(err)
	}
	if err := file.Sync(); err != nil {
		return ioFailure(err)
	}
	return nil
}
