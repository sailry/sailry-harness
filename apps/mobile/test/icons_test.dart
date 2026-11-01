import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/ui/icons.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('Reicon assets match declared viewports', () async {
    final manifest =
        jsonDecode(await rootBundle.loadString('assets/icons/sources.json'))
            as Map<String, dynamic>;
    final icons = manifest['icons'] as Map<String, dynamic>;
    expect(icons, isNotEmpty);
    for (final name in icons.keys) {
      final picture = await vg.loadPicture(
        SvgAssetLoader('assets/icons/$name.svg'),
        null,
      );
      expect(picture.size, const Size(24, 24), reason: name);
      picture.picture.dispose();
    }
  });

  test('catalog names resolve to their declared assets', () async {
    final manifest =
        jsonDecode(await rootBundle.loadString('assets/icons/sources.json'))
            as Map<String, dynamic>;
    final icons = manifest['icons'] as Map<String, dynamic>;
    expect(catalogIcons.values.toSet(), everyElement(isIn(icons.keys)));
    for (final MapEntry(key: name, value: path) in icons.entries) {
      expect(icons[catalogIcon('reicon:$path')], path, reason: name);
    }
    expect(catalogIcon('ui/check'), isNull);
    expect(catalogIcon('reicon:missing/icon'), isNull);
  });
}
