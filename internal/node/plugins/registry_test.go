package plugins

import (
	"archive/zip"
	"bytes"
	"crypto/ed25519"
	"crypto/md5"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRegistryVerification(t *testing.T) {
	t.Parallel()
	document := []byte("---\nname: review\ndescription: Review changes\n---\nReview")
	var archive bytes.Buffer
	writer := zip.NewWriter(&archive)
	file, err := writer.Create("SKILL.md")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.Write(document); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	tree := fmt.Sprintf("SKILL.md:%x\n", sha256.Sum256(document))
	payload := registryPayload{Version: 1, Issuer: "skillhub.cn", Slug: "review", SkillVersion: "0.1.0", VersionID: 1, PublisherID: 1,
		ContentHash: fmt.Sprintf("%x", sha256.Sum256([]byte(tree))), PackageMD5: fmt.Sprintf("%x", md5.Sum(archive.Bytes())), FileCount: 1}
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatal(err)
	}
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	envelope := registryEnvelope{Signed: true, KeyID: "fixture", Payload: string(body), Signature: base64.StdEncoding.EncodeToString(ed25519.Sign(private, body)), ContentHash: payload.ContentHash, HashVersion: 1}
	var keys registryKeys
	raw, err := json.Marshal(map[string]any{"keys": []map[string]string{{"key_id": "fixture", "algorithm": "Ed25519", "public_key_raw_b64": base64.StdEncoding.EncodeToString(public), "status": "active", "issuer": "skillhub.cn"}}})
	if err != nil || json.Unmarshal(raw, &keys) != nil {
		t.Fatal("key fixture is invalid")
	}
	verified, err := verifyRegistrySignature(envelope, keys, "review", "0.1.0")
	if err != nil {
		t.Fatal(err)
	}
	resolved, files, err := verifyRegistryArchive(t.Context(), archive.Bytes(), protocol.PluginSkillsRegistryResolved{}, verified)
	if err != nil || resolved.ArchiveSha256 == "" || !bytes.Equal(files["SKILL.md"], document) {
		t.Fatalf("verified archive unavailable: %#v %v", resolved, err)
	}
	envelope.Payload += " "
	if _, err := verifyRegistrySignature(envelope, keys, "review", "0.1.0"); err == nil {
		t.Fatal("changed signed payload was accepted")
	}
	tampered := bytes.Clone(archive.Bytes())
	tampered[len(tampered)/2] ^= 1
	if _, _, err := verifyRegistryArchive(t.Context(), tampered, protocol.PluginSkillsRegistryResolved{}, verified); err == nil {
		t.Fatal("changed archive bytes were accepted")
	}
	if source, err := RegistrySourceURL("https://skillhub.cn/skills/team/review?version=0.1.0"); err != nil || source == nil || source.Namespace == nil || *source.Namespace != "team" {
		t.Fatalf("registry URL did not resolve: %#v %v", source, err)
	}
}

func TestRegistrySkillTakesItsSlug(t *testing.T) {
	source := protocol.PluginSkillsRegistryResolved{Namespace: "team", Slug: "acme-analyzing-feedback", Version: "1.0.0"}
	for name, document := range map[string]string{
		"republished": "---\nname: analyzing-feedback\ndescription: Analyze feedback\n---\nBody\n",
		"display":     "---\nname: Analyzing Feedback\ndescription: Analyze feedback\n---\nBody\n",
	} {
		t.Run(name, func(t *testing.T) {
			data, err := normalizeRegistrySkill([]byte(document), source)
			if err != nil {
				t.Fatal(err)
			}
			skill, err := ParseSkill(data, source.Slug)
			if err != nil || skill.Name != source.Slug || skill.DisplayName == nil || *skill.DisplayName == "" {
				t.Fatalf("skill %+v, %v", skill, err)
			}
		})
	}
	if _, err := normalizeRegistrySkill([]byte("---\nname: x\nslug: other\ndescription: d\n---\n"), source); err == nil {
		t.Fatal("a document naming another slug is accepted")
	}
}
