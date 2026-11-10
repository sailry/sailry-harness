import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_launcher_icons/config/config.dart';
import 'package:flutter_test/flutter_test.dart';

(int, int) dimensions(String path) {
  final bytes = File(path).readAsBytesSync();
  expect(bytes.take(8), [137, 80, 78, 71, 13, 10, 26, 10], reason: path);
  final header = ByteData.sublistView(bytes);
  return (header.getUint32(16), header.getUint32(20));
}

void main() {
  test('launcher config reuses the desktop mark', () {
    final config = Config.loadConfigFromPubSpec('.')!;
    final source = File('../../assets/branding/sailry.png').absolute.path;
    expect(File(config.imagePath!).absolute.path, source);
    expect(File(config.adaptiveIconForeground!).absolute.path, source);
    expect(config.android, isTrue);
    expect(config.ios, isTrue);
    expect(config.removeAlphaIOS, isTrue);
    expect(config.backgroundColorIOS, '#ffffff');
    expect(config.adaptiveIconBackground, '#ffffff');
    expect(dimensions(config.imagePath!), (1024, 1024));
  });

  test('iOS catalog has every declared scale and opaque icons', () {
    const directory = 'ios/Runner/Assets.xcassets/AppIcon.appiconset';
    final catalog =
        jsonDecode(File('$directory/Contents.json').readAsStringSync())
            as Map<String, dynamic>;
    for (final image in catalog['images'] as List) {
      final size = double.parse((image['size'] as String).split('x').first);
      final scale = int.parse((image['scale'] as String).replaceAll('x', ''));
      final pixels = (size * scale).toInt();
      final path = '$directory/${image['filename']}';
      expect(dimensions(path), (pixels, pixels), reason: path);
      // PNG IHDR color type 2 is RGB, with no alpha channel.
      expect(File(path).readAsBytesSync()[25], 2, reason: path);
    }
  });

  test('Android legacy and adaptive density assets are generated', () {
    const directory = 'android/app/src/main/res';
    for (final (density, legacy, adaptive) in [
      ('mdpi', 48, 108),
      ('hdpi', 72, 162),
      ('xhdpi', 96, 216),
      ('xxhdpi', 144, 324),
      ('xxxhdpi', 192, 432),
    ]) {
      expect(dimensions('$directory/mipmap-$density/ic_launcher.png'), (
        legacy,
        legacy,
      ));
      expect(
        dimensions('$directory/drawable-$density/ic_launcher_foreground.png'),
        (adaptive, adaptive),
      );
    }
    final adaptive = File(
      '$directory/mipmap-anydpi-v26/ic_launcher.xml',
    ).readAsStringSync();
    expect(adaptive, contains('@color/ic_launcher_background'));
    expect(adaptive, contains('@drawable/ic_launcher_foreground'));
    expect(
      File('$directory/values/colors.xml').readAsStringSync(),
      contains('#ffffff'),
    );
  });
}
