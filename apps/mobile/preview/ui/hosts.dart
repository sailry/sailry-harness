import 'package:flutter/material.dart';
import '../l10n/strings.dart';
import 'package:sailry_mobile/ui/kit.dart';

Future<void> showHostPicker(
  BuildContext context, {
  required String selected,
  bool allowAll = false,
  List<String> additionalHosts = const [],
  VoidCallback? onAdd,
  required ValueChanged<String> onSelected,
}) async {
  final value = await showAppSheet<String>(
    context,
    context.tr('selectHost'),
    actions: onAdd == null
        ? []
        : [
            ListTile(
              leading: const AppIcon('plus'),
              title: Text(context.tr('pair')),
              onTap: () {
                Navigator.of(context).pop();
                onAdd();
              },
            ),
          ],
    child: Builder(
      builder: (sheetContext) => Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (allowAll)
            ListTile(
              title: Text(context.tr('allHosts')),
              trailing: selected == 'all' ? const AppIcon('check') : null,
              onTap: () => Navigator.pop(sheetContext, 'all'),
            ),
          for (final host in [
            'Studio',
            'Build Server',
            'MacBook Air',
            ...additionalHosts,
          ])
            ListTile(
              leading: Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: host == 'MacBook Air'
                      ? Theme.of(context).colorScheme.onSurfaceVariant
                      : Theme.of(context).colorScheme.tertiary,
                ),
              ),
              title: Text(host),
              subtitle: Text(
                context.tr(host == 'MacBook Air' ? 'offline' : 'online'),
              ),
              trailing: selected == host ? const AppIcon('check') : null,
              onTap: () => Navigator.pop(sheetContext, host),
            ),
        ],
      ),
    ),
  );
  if (value != null) onSelected(value);
}
