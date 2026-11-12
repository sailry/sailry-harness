import 'package:flutter/widgets.dart';

import '../../../l10n/strings.dart';
import '../../conversations/live/recency.dart';

/// When an entry last changed; null when the host did not say.
DateTime? entryModified(Map<String, dynamic> entry) {
  final modified = entry['modified_ms'];
  return modified is num && modified > 0
      ? DateTime.fromMillisecondsSinceEpoch(modified.toInt())
      : null;
}

/// An entry's modification time as the lists write times; null without one.
String? entryModifiedLabel(BuildContext context, Map<String, dynamic> entry) {
  final modified = entryModified(entry);
  return modified == null
      ? null
      : timeLabel(
          modified,
          DateTime.now(),
          locale: dateLocale(Localizations.localeOf(context)),
          yesterday: context.tr('yesterday'),
        );
}

/// A browser location's icon: its kind's, and a device's by its medium. A
/// kind this build does not know is a folder.
String locationIcon(Map<String, dynamic> location) =>
    switch (location['kind']) {
      'home' => 'user',
      'desktop' => 'laptop',
      'documents' => 'file',
      'downloads' => 'arrow-down',
      'applications' => 'grid',
      'volume' || 'root' => switch (location['medium']) {
        'external' => 'archive',
        'network' => 'globe',
        _ => 'database',
      },
      _ => 'folder',
    };
