# Attachment fixtures

`document.pdf` is a minimal single-page document. Its fixed-width cross-reference entries include intentional trailing spaces.

`audio.wav`, `audio.mp4`, and `audio.mp3` contain a generated 440 Hz tone. `video.mp4` and `video.webm` contain two generated blue frames. They have no personal or third-party source content. Run `bash tests/fixtures/media.sh` with FFmpeg to create missing fixtures; the script preserves existing files. Set `SAILRY_FFMPEG` to use a specific executable. Encoder versions may produce different bytes, so tests compare the committed fixture bytes rather than regenerated encodings.
