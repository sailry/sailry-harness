import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../resources/artifact/page.dart';
import '../../resources/artifact/submission.dart';

/// Published revisions remain visible outside collapsed tool activity.
List<Map<String, dynamic>> publishedArtifacts(
  List<Map<String, dynamic>> calls,
  String turn,
  String session, {
  String? origin,
}) {
  final seen = <(String, int)>{};
  final artifacts = <Map<String, dynamic>>[];
  for (final call in calls.where((call) => call['turn'] == turn)) {
    for (final block in objects(object(call['content'])['blocks'])) {
      if (block['kind'] != 'artifact') continue;
      final artifact = object(block['artifact']);
      final id = text(artifact['id']);
      final revision = artifact['revision'];
      if (artifact['session'] == (origin ?? session) &&
          artifact['turn'] == turn &&
          id.isNotEmpty &&
          revision is int &&
          revision > 0 &&
          seen.add((id, revision))) {
        artifacts.add(Map.unmodifiable(artifact));
      }
    }
  }
  return artifacts;
}

class PublishedArtifacts extends StatelessWidget {
  const PublishedArtifacts({
    super.key,
    required this.artifacts,
    required this.host,
    required this.enabled,
    this.session,
    this.actions,
    this.changes,
  });

  final List<Map<String, dynamic>> artifacts;
  final HostConnection host;
  final bool enabled;
  final String? session;
  final ArtifactActions? actions;
  final ValueListenable<Map<String, dynamic>>? changes;

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      for (final artifact in artifacts)
        Padding(
          key: ValueKey('artifact-${artifact['id']}-${artifact['revision']}'),
          padding: const EdgeInsets.only(bottom: 8),
          child: Card.outlined(
            margin: EdgeInsets.zero,
            child: ListTile(
              leading: const AppIcon('file'),
              title: Text(text(artifact['title'])),
              subtitle: Text(
                [
                  if (text(artifact['summary']).isNotEmpty)
                    text(artifact['summary']),
                  'v${artifact['revision']}',
                ].join(' · '),
              ),
              trailing: const AppIcon('chevron', size: 18),
              onTap: !enabled || !host.connected
                  ? null
                  : () => pushPage(
                      context,
                      ArtifactPage(
                        host: host,
                        artifact: artifact,
                        session: session,
                        actions: actions,
                        changes: changes,
                      ),
                    ),
            ),
          ),
        ),
    ],
  );
}
