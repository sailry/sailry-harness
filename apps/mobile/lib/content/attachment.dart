import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import '../runtime/json.dart';
import '../runtime/session.dart';
import '../ui/kit.dart';
import '../ui/toast.dart';
import 'image_preview.dart';
import 'markdown.dart';
import 'transfers.dart';

class AttachmentView extends StatefulWidget {
  const AttachmentView({
    super.key,
    required this.host,
    required this.attachment,
    this.image,
    this.session,
  }) : worktree = null,
       path = null;

  const AttachmentView.file({
    super.key,
    required this.host,
    required this.worktree,
    required this.path,
  }) : attachment = null,
       image = null,
       session = null;
  final HostConnection host;
  final Map<String, dynamic>? attachment;
  final Map<String, dynamic>? image;
  final String? session;
  final String? worktree;
  final String? path;
  Object get _source => (
    host,
    worktree,
    path,
    session,
    attachment?['id'],
    object(attachment?['spec'])['revision'],
    image?['entry'],
    image?['part'],
    image?['index'],
  );

  @override
  State<AttachmentView> createState() => _AttachmentViewState();
}

class _AttachmentViewState extends State<AttachmentView> {
  Uint8List? _bytes;
  bool _busy = false;
  String? _error;
  int _generation = 0;
  late bool _connected;

  Map<String, dynamic> get _spec => widget.path == null
      ? object(widget.attachment!['spec'])
      : {
          'name': widget.path!.split('/').last,
          'media_type': mediaType(widget.path!),
        };

  @override
  void initState() {
    super.initState();
    _connected = widget.host.connected;
    widget.host.addListener(_hostChanged);
    if (_isImage) _load();
  }

  bool get _isImage =>
      widget.image != null ||
      widget.path != null ||
      (_spec['media_type'] as String? ?? '').startsWith('image/');

  void _hostChanged() {
    final reconnected = !_connected && widget.host.connected;
    setState(() => _connected = widget.host.connected);
    if (reconnected && _isImage) _load();
  }

  @override
  void didUpdateWidget(covariant AttachmentView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.host != widget.host) {
      oldWidget.host.removeListener(_hostChanged);
      widget.host.addListener(_hostChanged);
      _connected = widget.host.connected;
    }
    if (oldWidget._source != widget._source) {
      _generation++;
      _bytes = null;
      _error = null;
      _busy = false;
      if (_isImage) _load();
    }
  }

  @override
  void dispose() {
    widget.host.removeListener(_hostChanged);
    super.dispose();
  }

  Future<void> _load() async {
    if (_busy || _bytes != null || !widget.host.connected) return;
    final generation = _generation;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final bytes = widget.path != null
          ? await downloadFile(widget.host, widget.worktree!, widget.path!)
          : await downloadAttachment(
              widget.host,
              widget.attachment!,
              image: widget.image,
              session: widget.session,
            );
      if (!mounted || generation != _generation) return;
      setState(() => _bytes = bytes);
    } catch (_) {
      if (mounted && generation == _generation) {
        setState(() => _error = tr('conversationDownloadFailed'));
      }
    } finally {
      if (mounted && generation == _generation) setState(() => _busy = false);
    }
  }

  Future<void> _open() async {
    if (_busy) return;
    final source = widget._source;
    await _load();
    if (!mounted || _bytes == null || widget._source != source) return;
    final bytes = _bytes!;
    final spec = _spec;
    final mime = spec['media_type'] as String? ?? '';
    final name = spec['name'] as String? ?? 'attachment.bin';
    if (_isImage) {
      await showImagePreview(
        context,
        MemoryImage(bytes),
        name: name,
        save: () => saveContent(bytes, name, mime),
      );
      return;
    }
    await showAppSheet(
      context,
      name,
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (mime.startsWith('text/') || mime == 'application/json')
              mime == 'text/markdown'
                  ? MarkdownContent(utf8.decode(bytes, allowMalformed: true))
                  : SelectableText(utf8.decode(bytes, allowMalformed: true)),
            const SizedBox(height: 12),
            FilledButton(
              onPressed: () async {
                try {
                  await saveContent(bytes, name, mime);
                } catch (_) {
                  if (context.mounted) {
                    showToast(context, tr('fileSaveFailed'));
                  }
                }
              },
              child: Text(tr('save')),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final spec = _spec;
    final name = spec['name'] as String? ?? tr('conversationAttachment');
    final bytes = _bytes;
    return LoadingOverlay(
      loading: _busy,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (!_isImage || (_bytes == null && !_busy && _error == null))
            OutlinedButton(
              onPressed: _busy || !widget.host.connected ? null : _open,
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const AppIcon('file', size: 15),
                  const SizedBox(width: 6),
                  Flexible(child: Text(name, overflow: TextOverflow.ellipsis)),
                ],
              ),
            ),
          if (_isImage && _busy)
            const SizedBox(height: 96, width: double.infinity),
          if (bytes != null && _isImage)
            ImageThumbnail(
              image: MemoryImage(bytes),
              name: name,
              save: () =>
                  saveContent(bytes, name, spec['media_type'] as String? ?? ''),
            ),
          if (_error != null && _isImage) Text(name),
          if (_error != null)
            Row(
              children: [
                Flexible(
                  child: Text(
                    _error!,
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                ),
                TextButton(
                  onPressed: _busy || !widget.host.connected ? null : _load,
                  child: Text(tr('retry')),
                ),
              ],
            ),
        ],
      ),
    );
  }
}
