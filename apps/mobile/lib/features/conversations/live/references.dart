import '../../resources/file_page.dart';
import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../content/paths.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import '../../resources/file_navigation.dart';
import '../../resources/live_files.dart';

class MessageReferences extends StatelessWidget {
  const MessageReferences({
    super.key,
    this.documents,
    required this.references,
    required this.host,
    required this.worktree,
  });
  final Map<String, FileDocument>? documents;
  final List<Map<String, dynamic>> references;
  final HostConnection host;
  final String worktree;

  @override
  Widget build(BuildContext context) => Wrap(
    spacing: 6,
    runSpacing: 6,
    children: [
      for (final reference in references)
        ActionChip(
          label: Text(text(reference['label'], tr('messageReference'))),
          avatar: AppIcon(
            object(reference['target'])['kind'] == 'directory'
                ? 'folder'
                : 'file',
            size: 16,
          ),
          onPressed: () async {
            final target = object(reference['target']);
            if (!['file', 'directory'].contains(target['kind'])) {
              await showAppSheet(
                context,
                text(reference['label'], tr('messageReference')),
                child: Text(tr('messageReferenceContext')),
              );
              return;
            }
            final tree = objects(
              host.snapshot['worktrees'],
            ).where((tree) => tree['id'] == worktree).firstOrNull;
            final path = resourcePath(
              Uri(path: text(target['data'])),
              tree?['path'] as String?,
              allowRoot: target['kind'] == 'directory',
            );
            if (path == null || !host.connected) {
              showToast(context, tr('fileLinkUnavailable'));
              return;
            }
            if (target['kind'] == 'directory') {
              await pushPage(
                context,
                LiveFilesPage(
                  hostId: host.id,
                  worktreeId: worktree,
                  initialDirectory: path,
                ),
              );
            } else {
              await openResourceFile(
                context,
                host,
                worktree,
                path,
                documents: documents,
              );
            }
          },
        ),
    ],
  );
}
