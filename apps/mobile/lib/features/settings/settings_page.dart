import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../l10n/language.dart';
import '../../ui/kit.dart';
import '../../ui/theme.dart';
import '../../ui/toast.dart';
import '../../runtime/session.dart';
import '../updates/presentation.dart';
import '../updates/service.dart';
import 'live.dart';
import 'speech_sheet.dart';
import 'providers/provider_list.dart';
import 'usage_page.dart';

class SettingsPage extends StatefulWidget {
  const SettingsPage({
    super.key,
    required this.onThemeChanged,
    required this.themeMode,
    this.brand = Brand.orange,
    this.onBrandChanged,
    this.notifications = true,
    this.onNotificationsChanged,
    this.language = AppLanguage.chinese,
    this.onLanguageChanged,
    this.updates,
  });

  final ValueChanged<ThemeMode> onThemeChanged;
  final ThemeMode themeMode;
  final Brand brand;
  final ValueChanged<Brand>? onBrandChanged;
  final bool notifications;
  final ValueChanged<bool>? onNotificationsChanged;
  final AppLanguage language;
  final ValueChanged<AppLanguage>? onLanguageChanged;
  final AppUpdates? updates;

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  void _providers() {
    final host = AppSession.maybeOf(context)?.selectedHost;
    if (host?.connected == true) {
      pushPage(context, ProvidersPage(host: host!));
    }
  }

  /// The theme as icon segments in its row, chosen in place: following the
  /// system, light or dark.
  Widget _appearance() {
    final colors = Theme.of(context).colorScheme;
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: 16),
      leading: AppIcon('sun', color: colors.onSurfaceVariant),
      title: Text(context.tr('appearance')),
      trailing: Segments<ThemeMode>(
        values: const [ThemeMode.system, ThemeMode.light, ThemeMode.dark],
        selected: widget.themeMode,
        icon: (mode) => switch (mode) {
          ThemeMode.system => 'monitor',
          ThemeMode.light => 'sun',
          ThemeMode.dark => 'moon',
        },
        label: (mode) =>
            context.tr(mode == ThemeMode.system ? 'auto' : mode.name),
        onChanged: widget.onThemeChanged,
      ),
    );
  }

  /// The brand color as swatches in its row, chosen in place.
  Widget _brand(ValueChanged<Brand> onChanged) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    return ListTile(
      contentPadding: const EdgeInsets.symmetric(horizontal: 16),
      leading: AppIcon('bulb', color: colors.onSurfaceVariant),
      title: Text(context.tr('brandColor')),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        spacing: 6,
        children: [
          for (final brand in Brand.values)
            Semantics(
              button: true,
              selected: brand == widget.brand,
              label: context.tr(brand.labelKey),
              excludeSemantics: true,
              child: Tooltip(
                message: context.tr(brand.labelKey),
                child: GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onTap: () => onChanged(brand),
                  child: AnimatedContainer(
                    duration: const Duration(milliseconds: 160),
                    width: 22,
                    height: 22,
                    padding: const EdgeInsets.all(3),
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      border: Border.all(
                        width: 1.5,
                        color: brand == widget.brand
                            ? brand.color(theme.brightness)
                            : Colors.transparent,
                      ),
                    ),
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        shape: BoxShape.circle,
                        color: brand.color(theme.brightness),
                      ),
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }

  void _speech() {
    final speech = AppSession.maybeOf(context)?.speech;
    if (speech != null) {
      showAppSheet(
        context,
        context.tr('speech'),
        child: SpeechSheet(speech: speech),
      );
    }
  }

  void _language() {
    showAppSheet(
      context,
      context.tr('language'),
      child: Builder(
        builder: (sheetContext) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final language in AppLanguage.values)
              ListTile(
                selected: widget.language == language,
                title: Text(context.tr(language.labelKey)),
                onTap: () {
                  widget.onLanguageChanged?.call(language);
                  Navigator.pop(sheetContext);
                },
              ),
          ],
        ),
      ),
    );
  }

  Widget _row(String icon, String label, VoidCallback? onTap, [String? value]) {
    final colors = Theme.of(context).colorScheme;
    final color = onTap == null
        ? Theme.of(context).disabledColor
        : colors.onSurfaceVariant;
    return LayoutBuilder(
      builder: (context, constraints) => ListTile(
        enabled: onTap != null,
        contentPadding: const EdgeInsets.symmetric(horizontal: 16),
        leading: AppIcon(icon, color: color),
        title: Text(context.tr(label)),
        trailing: ConstrainedBox(
          constraints: BoxConstraints(maxWidth: constraints.maxWidth / 2),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (value != null) ...[
                Flexible(
                  child: Text(
                    value,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: color),
                  ),
                ),
                const SizedBox(width: 8),
              ],
              if (onTap != null) AppIcon('chevron', size: 14, color: color),
            ],
          ),
        ),
        onTap: onTap,
      ),
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
      title: context.tr('settings'),
      actions: [
        RoundButton(
          icon: 'server',
          tooltip: '${context.tr('selectHost')}: ${host?.label ?? ''}',
          onPressed: session?.hosts.isNotEmpty == true
              ? () => pickSettingsHost(context, session!)
              : null,
        ),
      ],
      child: Column(
        children: [
          _group([
            _row(
              'globe',
              'language',
              widget.onLanguageChanged == null ? null : _language,
              context.tr(widget.language.labelKey),
            ),
            _appearance(),
            if (widget.onBrandChanged case final onChanged?) _brand(onChanged),
            SwitchTile(
              contentPadding: const EdgeInsets.symmetric(horizontal: 16),
              leading: AppIcon('bell', color: colors.onSurfaceVariant),
              title: Text(context.tr('completionAlerts')),
              value: widget.notifications,
              onChanged: widget.onNotificationsChanged,
            ),
            if (session?.background.supported == true)
              ListenableBuilder(
                listenable: session!.background,
                builder: (context, _) => SwitchTile(
                  contentPadding: const EdgeInsets.symmetric(horizontal: 16),
                  leading: AppIcon('server', color: colors.onSurfaceVariant),
                  title: Text(context.tr('backgroundConnection')),
                  value: session.background.enabled,
                  onChanged: (value) async {
                    try {
                      await session.background.setEnabled(value);
                    } catch (_) {
                      if (context.mounted) {
                        showToast(
                          context,
                          context.tr('backgroundConnectionFailed'),
                        );
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
              _row('spark', 'providers', connected ? _providers : null),
              _row(
                'mic',
                'speech',
                session?.speech == null || host?.isDemo == true
                    ? null
                    : _speech,
              ),
            ]),
          ),
          const SizedBox(height: 16),
          UpdateSettings(updates: host?.isDemo == true ? null : widget.updates),
        ],
      ),
    );
  }
}
