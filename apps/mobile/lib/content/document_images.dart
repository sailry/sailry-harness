import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import '../runtime/session.dart';
import 'image_preview.dart';
import 'paths.dart';
import 'transfers.dart';

/// A document's source is captured at open, independent of global navigation.
/// An imported attachment has no source unless its caller supplies one.
class DocumentImageSource {
  DocumentImageSource({
    required this.host,
    required this.worktree,
    required this.path,
    required this.version,
  });

  final HostConnection host;
  final String worktree;
  final String path;
  final Object? version;

  DocumentImageSource atVersion(Object? version) => DocumentImageSource(
    host: host,
    worktree: worktree,
    path: path,
    version: version,
  );

  Object get identity =>
      (host, host.entryId, host.nodeId, worktree, path, version);

  String? resolve(Uri uri) => documentImagePath(path, uri);
}

/// Local image URLs are relative to the captured document, with one URL decode.
/// General navigation links retain their separate absolute/file URL policy.
String? documentImagePath(String document, Uri reference) {
  final forbidden = RegExp(r'[\\:\x00-\x1f\x7f]');
  if (reference.hasScheme ||
      reference.hasAuthority ||
      reference.path.isEmpty ||
      document.isEmpty ||
      document.startsWith('/') ||
      forbidden.hasMatch(document)) {
    return null;
  }
  String relative;
  try {
    relative = Uri.decodeComponent(reference.path);
  } on FormatException {
    return null;
  }
  if (relative.isEmpty ||
      relative.startsWith('/') ||
      forbidden.hasMatch(relative)) {
    return null;
  }
  final parts = <String>[];
  for (final part in '${splitPath(document).directory}/$relative'.split('/')) {
    if (part.isEmpty || part == '.') continue;
    if (part == '..') {
      if (parts.isEmpty) return null;
      parts.removeLast();
    } else {
      parts.add(part);
    }
  }
  return parts.isEmpty ? null : parts.join('/');
}

/// A preview owns its image cache and download streams until it closes.
class DocumentImages extends ChangeNotifier {
  DocumentImages(this.source) {
    source?.host.addListener(_hostChanged);
    _connected = source?.host.connected == true;
  }

  final DocumentImageSource? source;
  final _cancellation = DownloadCancellation();
  final _reads = <Object, Future<DownloadedFile>>{};
  final _failed = <Object>{};
  bool _closed = false;
  bool _connected = false;
  int _cachedBytes = 0;
  Future<void> _pending = Future.value();

  Future<DownloadedFile> _load(Uri uri) {
    final origin = source;
    final path = origin?.resolve(uri);
    if (_closed || origin == null || path == null) {
      return Future.error(StateError('Image source unavailable'));
    }
    final key = (origin.identity, path);
    return _reads.putIfAbsent(key, () {
      // Serial image reads reserve the remaining existing transfer budget, so
      // concurrent images cannot each retain another maximum-sized transfer.
      final read = _pending.then((_) async {
        try {
          _cancellation.check();
          final remaining = maximumAttachmentBytes - _cachedBytes;
          if (remaining <= 0) throw StateError('Document image cache is full');
          final content = await downloadFileContent(
            origin.host,
            origin.worktree,
            path,
            cancellation: _cancellation,
            maximumBytes: remaining,
          );
          _cancellation.check();
          _cachedBytes += content.bytes.length;
          return content;
        } catch (_) {
          if (!_closed) _failed.add(key);
          rethrow;
        }
      });
      _pending = read.then<void>((_) {}, onError: (Object _, StackTrace _) {});
      return read;
    });
  }

  void _hostChanged() {
    final connected = source?.host.connected == true;
    if (_closed || connected == _connected) return;
    final reconnected = connected && !_connected;
    _connected = connected;
    if (reconnected) {
      for (final key in _failed) {
        _reads.remove(key);
      }
      _failed.clear();
    }
    notifyListeners();
  }

  Widget image(Uri uri, String? title, String? alt) {
    if (['http', 'https', 'data'].contains(uri.scheme)) return uriImage(uri);
    return ListenableBuilder(
      key: ValueKey((source?.identity, uri.toString())),
      listenable: this,
      builder: (context, _) => FutureBuilder<DownloadedFile>(
        future: _load(uri),
        builder: (context, snapshot) {
          final content = snapshot.data;
          if (content == null) {
            return Text(
              alt?.trim().isNotEmpty == true
                  ? alt!
                  : context.tr('conversationImageFailed'),
            );
          }
          final name = source?.resolve(uri) ?? uri.path;
          final mime = mediaType(name);
          return KeyedSubtree(
            key: ValueKey((
              source?.identity,
              name,
              content.revision,
              content.stamp,
            )),
            child: imageBytes(
              content.bytes,
              mime,
              name: title?.isNotEmpty == true ? title : name,
              save: () =>
                  saveContent(content.bytes, splitPath(name).name, mime),
            ),
          );
        },
      ),
    );
  }

  @override
  void dispose() {
    _closed = true;
    source?.host.removeListener(_hostChanged);
    _cancellation.cancel();
    _reads.clear();
    _cachedBytes = 0;
    _failed.clear();
    super.dispose();
  }
}
