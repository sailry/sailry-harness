import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';

void registerIconLicense() {
  LicenseRegistry.addLicense(() async* {
    final notice = await rootBundle.loadString('assets/icons/NOTICE.txt');
    final license = await rootBundle.loadString('assets/icons/LICENSE.txt');
    yield LicenseEntryWithLineBreaks(['Reicon'], '$notice\n$license');
    final monsters = await rootBundle.loadString(
      'third_party_licenses/kenney-monsters.md',
    );
    yield LicenseEntryWithLineBreaks(['Kenney Monster Builder'], monsters);
  });
}

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
