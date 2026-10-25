import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter_test/flutter_test.dart';

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
}
