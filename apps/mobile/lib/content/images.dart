import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import '../runtime/json.dart';
import '../runtime/session.dart';
import 'attachment.dart';
import 'image_preview.dart';
import 'paths.dart';

/// Resolve against the execution Node and the turn's captured worktree.
Widget contentImage(
  HostConnection host,
  String worktree,
  Uri uri, {
  String directory = '',
}) {
  if (['http', 'https', 'data'].contains(uri.scheme)) return uriImage(uri);
  final tree = objects(
    host.snapshot['worktrees'],
  ).where((tree) => tree['id'] == worktree).firstOrNull;
  final path = resourcePath(
    uri,
    tree?['path'] as String?,
    directory: directory,
  );
  if (path == null) {
    return Builder(
      builder: (context) => Text(context.tr('conversationImageFailed')),
    );
  }
  return AttachmentView.file(
    key: ValueKey('${host.id}:$worktree:$path'),
    host: host,
    worktree: worktree,
    path: path,
  );
}
