import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart' show AppSession, HostConnection;
export '../../runtime/json.dart' show object, objects, text, number, newId;
import '../../ui/kit.dart';

int integer(Object? value) => value is num ? value.toInt() : 0;
String string(Object? value) => value is String ? value : '';

String failure(Object error, {bool saving = false}) {
  final detail = error.toString();
  if (detail.contains('revision_conflict')) return tr('settingsConflict');
  if (detail.contains('outcome_unknown')) return tr('settingsUnknown');
  return tr(saving ? 'settingsSaveFailed' : 'settingsLoadFailed');
}

Future<void> pickSettingsHost(BuildContext context, AppSession session) async {
  final selected = await showAppSheet<String>(
    context,
    tr('selectHost'),
    child: Builder(
      builder: (context) => Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final host in session.hosts)
            ListTile(
              title: Text(host.label),
              leading: const AppIcon('server'),
              trailing: session.selectedHost?.id == host.id
                  ? const AppIcon('check')
                  : null,
              onTap: () => Navigator.pop(context, host.id),
            ),
        ],
      ),
    ),
  );
  if (selected != null) session.selectHost(selected);
}

Widget settingsError(String? error) => error == null
    ? const SizedBox.shrink()
    : Padding(
        padding: const EdgeInsets.symmetric(vertical: 12),
        child: Semantics(liveRegion: true, child: Text(error)),
      );

Future<bool> confirmRemoval(BuildContext context, String name) async =>
    await showAppSheet<bool>(
      context,
      tr('delete'),
      child: Builder(
        builder: (context) => Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(tr('configDelete').replaceAll('{name}', name)),
            const SizedBox(height: 20),
            Row(
              children: [
                Expanded(
                  child: OutlinedButton(
                    onPressed: () => Navigator.pop(context, false),
                    child: Text(tr('cancel')),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: FilledButton(
                    onPressed: () => Navigator.pop(context, true),
                    child: Text(tr('delete')),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    ) ==
    true;

/// A settings sheet captures its execution host; switching the shell's host never
/// redirects an in-progress save to another Node.
typedef SettingsHost = HostConnection;
