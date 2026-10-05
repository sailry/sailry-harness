import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';
import '../../runtime/session.dart';
import 'live.dart';
import 'memory_sheet.dart';
import 'speech_sheet.dart';
import 'setting_records.dart';
import 'usage_page.dart';

class SettingsPage extends StatefulWidget {
  const SettingsPage({
    super.key,
    required this.onThemeChanged,
    required this.themeMode,
    this.initialHost = 'Studio',
    this.onHostChanged,
    this.notifications = true,
    this.onNotificationsChanged,
  });

  final ValueChanged<ThemeMode> onThemeChanged;
  final ThemeMode themeMode;
  final String initialHost;
  final ValueChanged<String>? onHostChanged;
  final bool notifications;
  final ValueChanged<bool>? onNotificationsChanged;

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  void _records(String kind) {
    final host = AppSession.maybeOf(context)?.selectedHost;
    if (host?.connected == true) {
      showSettingRecords(context, kind: kind, host: host!);
    }
  }

  void _appearance() {
    showAppSheet(
      context,
      tr('appearance'),
      child: Builder(
        builder: (sheetContext) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final mode in [ThemeMode.light, ThemeMode.dark])
              ListTile(
                leading: AppIcon(mode == ThemeMode.dark ? 'moon' : 'sun'),
                title: Text(tr(mode.name)),
                trailing: Theme.of(context).brightness.name == mode.name
                    ? const AppIcon('check')
                    : null,
                onTap: () {
                  widget.onThemeChanged(mode);
                  Navigator.pop(sheetContext);
                },
              ),
          ],
        ),
      ),
    );
  }

  void _speech() {
    final speech = AppSession.maybeOf(context)?.speech;
    if (speech != null) {
      showAppSheet(context, tr('speech'), child: SpeechSheet(speech: speech));
    }
  }

  Widget _row(String icon, String label, VoidCallback? onTap, [String? value]) {
    final colors = Theme.of(context).colorScheme;
    final color = onTap == null
        ? Theme.of(context).disabledColor
        : colors.onSurfaceVariant;
    return ListTile(
      enabled: onTap != null,
      contentPadding: const EdgeInsets.symmetric(horizontal: 16),
      leading: AppIcon(icon, color: color),
      title: Text(tr(label)),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (value != null) ...[
            Text(value, style: TextStyle(color: color)),
            const SizedBox(width: 8),
          ],
          if (onTap != null) AppIcon('chevron', size: 14, color: color),
        ],
      ),
      onTap: onTap,
    );
  }

  Widget _group(List<Widget> children) => Surface(
    padding: const EdgeInsets.symmetric(vertical: 4),
    child: Column(
      children: [
        for (var index = 0; index < children.length; index++) ...[
          if (index > 0) const Divider(height: 1, indent: 16, endIndent: 16),
          children[index],
        ],
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final session = AppSession.maybeOf(context);
    final host = session?.selectedHost;
    final connected = host?.connected == true;
    return PageFrame(
      title: tr('settings'),
      actions: [
        RoundButton(
          icon: 'server',
          tooltip: '${tr('selectHost')}: ${host?.label ?? ''}',
          onPressed: session?.hosts.isNotEmpty == true
              ? () => pickSettingsHost(context, session!)
              : null,
        ),
      ],
      child: Column(
        children: [
          _group([
            _row(
              'sun',
              'appearance',
              _appearance,
              tr(Theme.of(context).brightness.name),
            ),
            SwitchListTile.adaptive(
              contentPadding: const EdgeInsets.symmetric(horizontal: 16),
              secondary: AppIcon('bell', color: colors.onSurfaceVariant),
              title: Text(tr('completionAlerts')),
              value: widget.notifications,
              onChanged: widget.onNotificationsChanged,
            ),
            if (session?.background.supported == true)
              ListenableBuilder(
                listenable: session!.background,
                builder: (context, _) => SwitchListTile.adaptive(
                  contentPadding: const EdgeInsets.symmetric(horizontal: 16),
                  secondary: AppIcon('server', color: colors.onSurfaceVariant),
                  title: Text(tr('backgroundConnection')),
                  value: session.background.enabled,
                  onChanged: (value) async {
                    try {
                      await session.background.setEnabled(value);
                    } catch (_) {
                      if (context.mounted) {
                        showToast(context, tr('backgroundConnectionFailed'));
                      }
                    }
                  },
                ),
              ),
            _row(
              'chart',
              'usage',
              session?.hosts.any((host) => host.connected) == true
                  ? () => pushPage(context, const UsagePage())
                  : null,
            ),
          ]),
          const SizedBox(height: 16),
          Semantics(
            label: host?.label ?? '',
            child: _group([
              _row(
                'spark',
                'providers',
                connected ? () => _records('providers') : null,
              ),
              _row('user', 'roles', connected ? () => _records('roles') : null),
              _row(
                'file',
                'memorySettings',
                !connected
                    ? null
                    : () => showAppSheet(
                        context,
                        tr('memorySettings'),
                        scroll: false,
                        child: MemorySheet(host: host!),
                      ),
              ),
              _row('mic', 'speech', session?.speech == null ? null : _speech),
            ]),
          ),
        ],
      ),
    );
  }
}
