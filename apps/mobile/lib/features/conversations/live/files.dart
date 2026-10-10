import 'package:flutter/material.dart';

import '../../../content/file_size.dart';
import '../../../content/transfers.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import '../../resources/file_navigation.dart';
import '../../resources/file_page.dart';

/// Newer publications replace the same path while retaining transcript order.
List<Map<String, dynamic>> publishedFiles(
  List<Map<String, dynamic>> calls,
  String turn,
) {
  final paths = <String>{};
  final files = <Map<String, dynamic>>[];
  for (final call in calls.reversed.where((call) => call['turn'] == turn)) {
    for (final block in objects(object(call['content'])['blocks'])) {
      if (block['kind'] == 'file' && paths.add(text(block['path']))) {
        files.add(block);
      }
    }
  }
  return files.reversed.toList();
}

class PublishedFiles extends StatelessWidget {
  const PublishedFiles({
    super.key,
    required this.files,
    required this.host,
    required this.worktree,
    required this.enabled,
    this.documents,
  });

  final List<Map<String, dynamic>> files;
  final HostConnection host;
  final String worktree;
  final bool enabled;
  final Map<String, FileDocument>? documents;

  Future<void> _open(BuildContext context, Map<String, dynamic> file) =>
      openResourceFile(
        context,
        host,
        worktree,
        text(file['path']),
        documents: documents,
      );

  Future<void> _save(BuildContext context, Map<String, dynamic> file) async {
    final path = text(file['path']);
    final name = text(file['name'], path.split('/').last);
    final mime = text(file['mime']);
    try {
      final bytes = await downloadFile(host, worktree, path);
      if (context.mounted) await saveContent(bytes, name, mime);
    } catch (_) {
      if (context.mounted) showToast(context, context.tr('fileSaveFailed'));
    }
  }

  Future<void> _actions(BuildContext context, Map<String, dynamic> file) =>
      showAppSheet(
        context,
        text(file['name'], text(file['path']).split('/').last),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              leading: const AppIcon('file'),
              title: Text(context.tr('preview')),
              onTap: () {
                Navigator.pop(context);
                _open(context, file);
              },
            ),
            ListTile(
              leading: const AppIcon('arrow-down'),
              title: Text(context.tr('save')),
              onTap: () {
                Navigator.pop(context);
                _save(context, file);
              },
            ),
          ],
        ),
      );

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      for (final file in files)
        Padding(
          key: ValueKey('published-${file['path']}'),
          padding: const EdgeInsets.only(bottom: 8),
          child: Card.outlined(
            margin: EdgeInsets.zero,
            child: ListTile(
              leading: const AppIcon('file'),
              title: Text(
                text(file['name'], text(file['path']).split('/').last),
              ),
              subtitle: Text(
                [
                  text(file['mime']),
                  if (file['size'] is int) fileSize(file['size'] as int),
                ].where((value) => value.isNotEmpty).join(' · '),
              ),
              trailing: IconButton(
                tooltip: context.tr('more'),
                icon: const AppIcon('more', size: 18),
                onPressed: !enabled || !host.connected
                    ? null
                    : () => _actions(context, file),
              ),
              onTap: !enabled || !host.connected
                  ? null
                  : () => _open(context, file),
            ),
          ),
        ),
    ],
  );
}
