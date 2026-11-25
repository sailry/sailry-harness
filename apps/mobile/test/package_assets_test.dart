import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/artifact/content.dart';
import 'package:sailry_mobile/ui/package_assets.dart';

class _Assets extends CachingAssetBundle {
  final keys = <String>[];

  @override
  Future<ByteData> load(String key) async {
    keys.add(key);
    return ByteData.sublistView(utf8.encode('window.fixture = true;'));
  }
}

void main() {
  test(
    'shared assets use the dependency namespace and retain bundle caching',
    () async {
      final source = _Assets();
      final assets = PackageAssetBundle('sailry_mobile', source: source);
      const key = 'assets/icons/NOTICE.txt';
      expect(await assets.loadString(key), 'window.fixture = true;');
      expect(await assets.loadString(key), 'window.fixture = true;');
      expect(source.keys, ['packages/sailry_mobile/$key']);
    },
  );

  test('artifact libraries load from the product-owned bundle', () async {
    final assets = _Assets();
    final page = await artifactPage(
      {
        'html': '<p>Fixture</p>',
        'view': {
          'template': '<html><head></head><body></body></html>',
          'libraries': ['mermaid'],
        },
      },
      {},
      {},
      writable: false,
      assets: assets,
    );
    expect(assets.keys, ['assets/visuals/mermaid.js']);
    expect(page, contains('window.fixture = true;'));
    expect(page, contains("connect-src 'none'"));
  });

  test(
    'root manifests and other package assets keep their original owner',
    () async {
      final source = _Assets();
      final assets = PackageAssetBundle('sailry_mobile', source: source);
      await assets.load('AssetManifest.bin');
      await assets.load('packages/another_package/assets/icon.svg');
      expect(source.keys, [
        'AssetManifest.bin',
        'packages/another_package/assets/icon.svg',
      ]);
    },
  );
}
