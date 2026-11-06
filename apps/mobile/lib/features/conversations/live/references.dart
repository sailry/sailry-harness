import '../../resources/file_page.dart';
import 'package:flutter/material.dart';

import '../../../content/attachment.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../content/paths.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import '../../resources/file_navigation.dart';
import '../../resources/files_page.dart';

class MessageReferences extends StatelessWidget {
  const MessageReferences({
    super.key,
    this.documents,
    required this.references,
    this.attachments = const [],
    required this.host,
    required this.worktree,
  });
  final Map<String, FileDocument>? documents;
  final List<Map<String, dynamic>> references;
  final List<Map<String, dynamic>> attachments;
  final HostConnection host;
  final String worktree;

  static bool _selects(Map<String, dynamic> reference) =>
      object(reference['target'])['kind'] == 'attachment';

  /// Attachment references sharing a label present one selection.
  List<Map<String, dynamic>> get _chips {
    final labels = <String>{};
    return [
      for (final reference in references)
        if (!_selects(reference) || labels.add(text(reference['label'])))
          reference,
    ];
  }

  List<Map<String, dynamic>> _selection(Map<String, dynamic> reference) {
    final ids = {
      for (final item in references)
        if (_selects(item) && item['label'] == reference['label'])
          object(item['target'])['data'],
    };
    return attachments.where((item) => ids.contains(item['id'])).toList();
  }

  String _icon(Map<String, dynamic> reference) {
    if (_selects(reference)) {
      return _selection(reference).any(
            (item) =>
                text(object(item['spec'])['media_type']).startsWith('image/'),
          )
          ? 'image'
          : 'file';
    }
    return object(reference['target'])['kind'] == 'directory'
        ? 'folder'
        : 'file';
  }

  @override
  Widget build(BuildContext context) => Wrap(
    spacing: 6,
    runSpacing: 6,
    children: [
      for (final reference in _chips)
        ActionChip(
          label: Text(text(reference['label'], context.tr('messageReference'))),
          avatar: AppIcon(_icon(reference), size: 16),
          onPressed: () async {
            final target = object(reference['target']);
            final selection = _selects(reference)
                ? _selection(reference)
                : const <Map<String, dynamic>>[];
            if (selection.isNotEmpty) {
              await showAppSheet(
                context,
                text(reference['label'], context.tr('messageReference')),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    for (final attachment in selection)
                      Padding(
                        padding: const EdgeInsets.only(bottom: 8),
                        child: AttachmentView(
                          host: host,
                          attachment: attachment,
                        ),
                      ),
                  ],
                ),
              );
              return;
            }
            if (!['file', 'directory'].contains(target['kind'])) {
              await showAppSheet(
                context,
                text(reference['label'], context.tr('messageReference')),
                child: Text(context.tr('messageReferenceContext')),
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
              showToast(context, context.tr('fileLinkUnavailable'));
              return;
            }
            if (target['kind'] == 'directory') {
              await pushPage(
                context,
                FilesPage(
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
