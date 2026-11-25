import 'package:flutter/material.dart';

import '../../../content/attachment.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../resources/file_draft.dart';
import 'links.dart';

class MessageReferences extends StatelessWidget {
  const MessageReferences({
    super.key,
    this.documents,
    required this.references,
    this.attachments = const [],
    required this.host,
    required this.worktree,
    this.session,
  });
  final Map<String, FileDocument>? documents;
  final List<Map<String, dynamic>> references;
  final List<Map<String, dynamic>> attachments;
  final HostConnection host;
  final String worktree;
  final String? session;

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
            await ConversationLinks(
              host,
              worktree,
              session: session,
              documents: documents,
            ).openReference(
              context,
              target,
              fallback: () => showAppSheet<void>(
                context,
                text(reference['label'], context.tr('messageReference')),
                child: selection.isEmpty
                    ? Text(context.tr('messageReferenceContext'))
                    : Column(
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
              ),
            );
          },
        ),
    ],
  );
}
