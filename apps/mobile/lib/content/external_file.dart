import 'dart:io';
import 'dart:typed_data';
import 'dart:ui';

import 'package:open_filex/open_filex.dart';
import 'package:path_provider/path_provider.dart';
import 'package:share_plus/share_plus.dart';

import '../l10n/strings.dart';
import 'transfers.dart';

/// Only verified downloads enter this cache, never execution-node paths.
Future<File> cacheExternalFile(Uint8List bytes, String name) async {
  final temporary = await getTemporaryDirectory();
  final cache = await Directory(
    '${temporary.path}/sailry-file-exports',
  ).create();
  final cutoff = DateTime.now().subtract(const Duration(days: 1));
  await for (final entry in cache.list(followLinks: false)) {
    try {
      if ((await entry.stat()).modified.isBefore(cutoff)) {
        await entry.delete(recursive: true);
      }
    } on FileSystemException {
      // An older export may still be in use by another application.
    }
  }
  final directory = await cache.createTemp('export-');
  final safeName = name.replaceAll(RegExp(r'[\\/:*?"<>|\x00-\x1f]'), '_');
  if (safeName.isEmpty || safeName == '.' || safeName == '..') {
    throw ArgumentError.value(name, 'name', 'Invalid file name');
  }
  // Keep the file after the native sheet closes: receivers may read it later.
  return File('${directory.path}/$safeName').writeAsBytes(bytes, flush: true);
}

Future<void> shareFile(File file, String name, Rect origin) async {
  await SharePlus.instance.share(
    ShareParams(
      files: [XFile(file.path, mimeType: mediaType(name))],
      fileNameOverrides: [name],
      sharePositionOrigin: origin,
    ),
  );
}

class FileOpenFailure implements Exception {
  FileOpenFailure(this.result);
  final ResultType result;

  String get message => tr(messageKey);
  String get messageKey =>
      result == ResultType.noAppToOpen ? 'fileNoApplication' : 'fileOpenFailed';
}

Future<void> openFile(
  File file,
  String name, {
  Future<OpenResult> Function(String, {String? type}) launch = OpenFilex.open,
}) async {
  final mime = mediaType(name);
  final result = await launch(
    file.path,
    type: mime == 'application/octet-stream' ? null : mime,
  );
  if (result.type != ResultType.done) {
    throw FileOpenFailure(result.type);
  }
}
