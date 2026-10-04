import 'package:flutter/material.dart';

import '../../runtime/session.dart';
import '../../ui/kit.dart';
import 'file_page.dart';

/// One navigation flow retains drafts and uncertain requests across linked files.
Future<void> openResourceFile(
  BuildContext context,
  HostConnection host,
  String worktree,
  String path, {
  Map<String, FileDocument>? documents,
}) async {
  final retained = documents ?? <String, FileDocument>{};
  final key = '${host.id}/$worktree/$path';
  final deleted = await pushPage<bool>(
    context,
    ResourceFilePage(
      host: host,
      worktree: worktree,
      path: path,
      document: retained.putIfAbsent(key, FileDocument.new),
      onOpenFile: (path) =>
          openResourceFile(context, host, worktree, path, documents: retained),
    ),
  );
  if (deleted == true) retained.remove(key);
}
