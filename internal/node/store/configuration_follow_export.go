package store

import (
	"encoding/hex"
	"encoding/json"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func configurationDigest(value any) (string, error) {
	body, err := encode(value)
	if err != nil {
		return "", err
	}
	sum := blake3.Sum256(body)
	return hex.EncodeToString(sum[:]), nil
}

func configurationManifestDigest(records []protocol.ConfigurationFollowRecord) (string, error) {
	metadata := make([]protocol.ConfigurationFollowRecord, len(records))
	for index, record := range records {
		metadata[index] = protocol.ConfigurationFollowRecord{ID: record.ID, Revision: record.Revision, Digest: record.Digest}
	}
	slices.SortFunc(metadata, func(a, b protocol.ConfigurationFollowRecord) int { return strings.Compare(a.ID, b.ID) })
	return configurationDigest(metadata)
}

func decodeConfigurationRecord(record protocol.ConfigurationFollowRecord, value any) error {
	if len(record.Body) == 0 {
		return invalid("changed configuration record body is required")
	}
	if err := json.Unmarshal(record.Body, value); err != nil {
		return invalid("configuration record body is invalid")
	}
	expected := record.Digest
	record.Digest = ""
	digest, err := configurationDigest(record)
	if err != nil {
		return err
	}
	if digest != expected {
		return invalid("configuration record digest does not match its content")
	}
	return nil
}
