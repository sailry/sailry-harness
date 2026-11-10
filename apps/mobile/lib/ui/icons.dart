import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';

void registerIconLicense() {
  LicenseRegistry.addLicense(() async* {
    final notice = await rootBundle.loadString('assets/icons/NOTICE.txt');
    final license = await rootBundle.loadString('assets/icons/LICENSE.txt');
    yield LicenseEntryWithLineBreaks(['Reicon'], '$notice\n$license');
    final lucide = await rootBundle.loadString(
      'assets/icons/LICENSE-LUCIDE.txt',
    );
    yield LicenseEntryWithLineBreaks(['Lucide'], lucide);
    final monsters = await rootBundle.loadString(
      'third_party_licenses/kenney-monsters.md',
    );
    yield LicenseEntryWithLineBreaks(['Kenney Monster Builder'], monsters);
  });
}

/// Bundled assets by Reicon catalog path, so captured `reicon:` declarations
/// can render without another icon set. Mirrors `assets/icons/sources.json`.
const catalogIcons = <String, String>{
  'arrows-action/reply2': 'enter',
  'arrows/arrow-down': 'arrow-down',
  'arrows/arrow-left': 'arrow-left',
  'arrows/arrow-right': 'arrow',
  'arrows/arrow-up': 'arrow-up',
  'arrows/chevron-down': 'down',
  'arrows/chevron-left': 'back',
  'arrows/chevron-right': 'chevron',
  'arrows/refresh': 'refresh',
  'business/chart-bar': 'chart',
  'devices/cpu': 'cpu',
  'devices/keyboard': 'keyboard',
  'devices/laptop': 'laptop',
  'devices/server': 'server',
  'files/file-text': 'file',
  'folders/folder': 'folder',
  'it/code': 'code',
  'it/station2': 'signal',
  'it/terminal-square': 'terminal',
  'map/globe': 'globe',
  'messages/chat-round-line': 'chat',
  'messages/pen': 'edit',
  'newicons/hierarchy2': 'branch',
  'notes/archive': 'archive',
  'notes/archive-up': 'restore',
  'notes/clipboard-check': 'task',
  'notifications/bell': 'bell',
  'search/search': 'search',
  'security/eye': 'eye',
  'security/shield': 'shield',
  'settings/settings': 'settings',
  'text-formatting/link': 'link',
  'time/clock': 'clock',
  'ui/battery-full': 'battery',
  'ui/check': 'check',
  'ui/copy': 'copy',
  'ui/database': 'database',
  'ui/expand': 'expand',
  'ui/grid': 'grid',
  'ui/info-circle': 'info',
  'ui/lightbulb': 'bulb',
  'ui/more-h': 'more',
  'ui/plus': 'plus',
  'ui/puzzle-piece': 'tool',
  'ui/sparkle': 'spark',
  'ui/trash': 'trash',
  'ui/wifi': 'wifi',
  'ui/x': 'close',
  'users/user': 'user',
  'video/image': 'image',
  'video/microphone': 'mic',
  'video/stop': 'stop',
  'weather/moon': 'moon',
  'weather/sun': 'sun',
};

/// Resolves a captured `reicon:` name to a bundled asset.
String? catalogIcon(String name) => name.startsWith('reicon:')
    ? catalogIcons[name.substring('reicon:'.length)]
    : null;

class AppIcon extends StatelessWidget {
  const AppIcon(this.name, {super.key, double? size, this.color})
    : size =
          size ??
          (name == 'down' || name == 'chevron' || name == 'back' ? 14 : 20);

  final String name;
  final double size;
  final Color? color;

  @override
  Widget build(BuildContext context) => SvgPicture.asset(
    'assets/icons/$name.svg',
    width: size,
    height: size,
    colorFilter: ColorFilter.mode(
      color ??
          IconTheme.of(context).color ??
          Theme.of(context).colorScheme.onSurfaceVariant,
      BlendMode.srcIn,
    ),
  );
}

/// Uses the native disclosure state instead of maintaining another expanded flag.
class DisclosureIcon extends StatelessWidget {
  const DisclosureIcon({super.key});

  @override
  Widget build(BuildContext context) {
    final controller = ExpansibleController.of(context);
    return ListenableBuilder(
      listenable: controller,
      builder: (context, child) => AnimatedRotation(
        turns: controller.isExpanded ? .5 : 0,
        duration: MediaQuery.disableAnimationsOf(context)
            ? Duration.zero
            : kThemeAnimationDuration,
        child: child,
      ),
      child: const AppIcon('down'),
    );
  }
}
